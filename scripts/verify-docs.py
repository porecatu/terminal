#!/usr/bin/env python3
# SPDX-License-Identifier: GPL-3.0-or-later
"""
Verificação de consistência da documentação do Porecatu.

Roda no CI (.github/workflows/docs.yml) e localmente:

    python scripts/verify-docs.py

Sem dependências além da biblioteca padrão. Requer Python 3.11+ (tomllib).
Sai com código 1 se qualquer checagem falhar.

As checagens existem porque a documentação faz três promessas que só um
script consegue manter honestas ao longo do tempo:

  1. Os links entre PRDs, ADRs e design resolvem.
  2. Os valores de aparência do porecatu.example.toml têm origem declarada
     na especificação visual — nenhuma cor inventada (CLAUDE.md, ADR-0009).
  3. Todo elemento do design está classificado [v1] ou [v2] — nenhum
     implementador fica sem saber se algo é escopo de agora.
  4. Os valores de aparência que o binário desenha são os mesmos que o
     example.toml traz como default e que a especificação registra —
     porque o binário é a referência visual (ADR-0028) e uma
     especificação que descreve o código defasa em silêncio.
"""

from __future__ import annotations

import glob
import os
import re
import sys
import tomllib
import urllib.parse

# Console do Windows pode vir em codepage legado; sem isto, acentos e setas
# levantam UnicodeEncodeError em vez de imprimir.
for fluxo in (sys.stdout, sys.stderr):
    if hasattr(fluxo, "reconfigure"):
        fluxo.reconfigure(encoding="utf-8", errors="replace")

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
SPEC = "docs/design/especificacao-visual.md"
CONFIG = "docs/config/porecatu.example.toml"
UI = "crates/porecatu-ui/src"

falhas: list[str] = []


def secao(titulo: str) -> None:
    print(f"\n=== {titulo} ===")


def ok(msg: str) -> None:
    print(f"  ok   {msg}")


def erro(msg: str) -> None:
    print(f"  ERRO {msg}")
    falhas.append(msg)


def ler(caminho: str) -> str:
    with open(caminho, encoding="utf-8") as f:
        return f.read()


# ---------------------------------------------------------------------------
# 1. Links relativos
# ---------------------------------------------------------------------------
def verificar_links() -> None:
    secao("Links relativos")
    quebrados = []
    total = 0
    for arquivo in glob.glob("**/*.md", recursive=True):
        base = os.path.dirname(arquivo) or "."
        for alvo in re.findall(r"\]\(([^)]+)\)", ler(arquivo)):
            if alvo.startswith(("http://", "https://", "mailto:", "#")):
                continue
            # Caminhos podem vir percent-encoded, como
            # "Terminal%20Multiplataforma.dc.html".
            destino = urllib.parse.unquote(alvo.split("#")[0])
            if not destino:
                continue
            total += 1
            if not os.path.exists(os.path.join(base, destino)):
                quebrados.append(f"{arquivo} -> {alvo}")

    if quebrados:
        for q in quebrados:
            erro(f"link quebrado: {q}")
    else:
        ok(f"{total} links relativos resolvem")


# ---------------------------------------------------------------------------
# 2. Configuração de exemplo
# ---------------------------------------------------------------------------
def carregar_config() -> dict | None:
    secao("Configuração de exemplo")
    if not os.path.exists(CONFIG):
        erro(f"{CONFIG} não encontrado")
        return None
    try:
        with open(CONFIG, "rb") as f:
            dados = tomllib.load(f)
    except tomllib.TOMLDecodeError as e:
        erro(f"{CONFIG} não parseia: {e}")
        return None
    ok(f"{CONFIG} parseia; seções: {', '.join(dados)}")
    return dados


# ---------------------------------------------------------------------------
# 3. Tokens órfãos
# ---------------------------------------------------------------------------
def folhas(d: dict, prefixo: str = ""):
    """Percorre o TOML devolvendo pares (caminho, valor) das folhas."""
    for chave, valor in d.items():
        caminho = f"{prefixo}.{chave}" if prefixo else chave
        if isinstance(valor, dict):
            yield from folhas(valor, caminho)
        elif isinstance(valor, list):
            for i, item in enumerate(valor):
                if isinstance(item, dict):
                    yield from folhas(item, f"{caminho}[{i}]")
                else:
                    yield caminho, item
        else:
            yield caminho, valor


def verificar_tokens(config: dict | None) -> None:
    secao("Tokens: origem na especificação visual")
    if config is None:
        erro("pulado — configuração não carregou")
        return
    if not os.path.exists(SPEC):
        erro(f"{SPEC} não encontrado")
        return

    spec_hex = {h.lower() for h in re.findall(r"#[0-9a-fA-F]{6}", ler(SPEC))}

    cores = [
        (c, v)
        for c, v in folhas(config)
        if isinstance(v, str) and re.fullmatch(r"#[0-9a-fA-F]{6}", v)
    ]
    # Temas alternativos (catppuccin, gruvbox) são paletas de terceiros:
    # não vêm do design e não devem ser rastreadas contra ele.
    tema = [c for c, _ in cores if c.startswith("themes")]
    principal = [(c, v) for c, v in cores if not c.startswith("themes")]
    orfas = [(c, v) for c, v in principal if v.lower() not in spec_hex]

    if orfas:
        for caminho, valor in orfas:
            erro(f"cor sem origem em {SPEC}: {caminho} = {valor}")
    else:
        ok(
            f"{len(principal)} cores rastreiam à especificação "
            f"({len(tema)} em temas alternativos, não rastreadas)"
        )


# ---------------------------------------------------------------------------
# 4. Cobertura da tabela de fases
# ---------------------------------------------------------------------------
def verificar_fases() -> None:
    secao("Tabela de fases do design")
    if not os.path.exists(SPEC):
        erro(f"{SPEC} não encontrado")
        return
    spec = ler(SPEC)

    classificados = re.findall(
        r"^\|\s*(?:\*\*)?([^|*][^|]*?)(?:\*\*)?\s*\|\s*`(\[v[12]\])`", spec, re.M
    )
    v1 = [n for n, f in classificados if f == "[v1]"]
    v2 = [n for n, f in classificados if f == "[v2]"]

    if not classificados:
        erro("nenhum elemento classificado — a tabela de fases sumiu?")
        return

    # Toda seção de anatomia precisa dizer a que fase pertence.
    sem_etiqueta = [
        linha
        for linha in spec.splitlines()
        if linha.startswith("### 2.") and "`[v" not in linha
    ]
    if sem_etiqueta:
        for linha in sem_etiqueta:
            erro(f"seção de anatomia sem etiqueta de fase: {linha.strip()}")
    else:
        ok(f"{len(classificados)} elementos classificados ({len(v1)} v1, {len(v2)} v2)")
        ok("nenhuma seção de anatomia sem etiqueta")


# ---------------------------------------------------------------------------
# 5. Valores: código, TOML e especificação
#
# O ADR-0028 fez do binário a referência visual, e a especificação passou a
# descrevê-lo. Isso cria um risco que os outros checks não cobrem: mudar uma
# constante em porecatu-ui e deixar documento e arquivo de exemplo para trás,
# sem nada quebrar. Esta checagem amarra os três lados dos valores que definem
# a forma da barra.
#
# A lista é EXPLÍCITA e cresce à mão. Não é um extrator genérico de constante
# — seriam ~210 delas, a maioria sem chave nem prosa correspondente (ver o
# cabeçalho do example.toml sobre o que não é configurável). O que ela garante
# é que os valores estruturais listados aqui nunca divergem em silêncio; e
# constante renomeada reprova, em vez de passar como "não encontrada".
# ---------------------------------------------------------------------------

# (rótulo, arquivo em UI, constante, chave do TOML, trecho esperado na espec)
VALORES = [
    ("tab_height", "tab_bar.rs", "tab_height", "appearance.tabs.tab_height",
     "**34px** (`tab_height`)"),
    ("trilha_padding", "tab_bar.rs", "trilha_padding",
     "appearance.tabs.trilha_padding", "`trilha_padding` **6px nos quatro lados**"),
    ("max_width", "tab_bar.rs", "max_width", "appearance.tabs.max_width",
     "`max_width` 260"),
    ("padding_left", "tab_bar.rs", "padding_left",
     "appearance.tabs.padding_left", "`padding: 0 6px 0 10px`"),
    ("gap entre abas", "tab_bar.rs", "tab_gap", "appearance.tabs.gap", "`gap: 4`"),
    ("icon_button_padding_x", "tab_bar.rs", "icon_button_padding_x",
     "appearance.tabs.icon_button_padding_x",
     "`icon_button_padding_x` **4px de cada lado**"),
    ("font_size da aba", "tab_bar.rs", "font_size", "appearance.tabs.font_size",
     "13px"),
    ("label_font_size", "tab_bar.rs", "pill_font_size",
     "appearance.groups.label_font_size", "13px/**500**"),
    ("label_max_width", "tab_bar.rs", "pill_name_max_width",
     "appearance.groups.label_max_width", "**140px** (`pill_name_max_width`)"),
    ("wrapper_padding", "tab_bar.rs", "wrapper_padding",
     "appearance.groups.wrapper_padding", "`padding: 3` (`wrapper_padding`)"),
    ("gap entre grupos", "tab_bar.rs", "trilha_gap", "appearance.groups.gap",
     "`gap: 6` (`trilha_gap`)"),
    ("indicador de overflow", "tab_bar.rs", "OVERFLOW_PILL_WIDTH",
     "appearance.tabs.overflow.indicator_size", "**18×18**"),
    ("recuo do overflow", "tab_bar.rs", "OVERFLOW_EDGE_INSET",
     "appearance.tabs.overflow.edge_inset", None),
    ("passo do overflow", "tab_bar.rs", "OVERFLOW_SCROLL_STEP",
     "appearance.tabs.overflow.scroll_step", "90 px"),
    ("botão de janela", "tab_bar.rs", "WINDOW_BUTTON_WIDTH",
     "appearance.window_controls.button_width", "**46px**"),
    ("semáforo do macOS", "tab_bar.rs", "MACOS_TRAFFIC_LIGHT_INSET",
     "appearance.window_controls.macos_traffic_light_inset", "78px"),
    ("em dos ícones", "chrome.rs", "ICON_EM_SIZE",
     "appearance.tabs.icon_em_size", "**20px de em**"),
    ("alfa da cápsula", "chrome.rs", "GROUP_CAPSULE_FILL_STRENGTH",
     "appearance.groups.capsule_alpha", "`.85` da cor cheia"),
    ("alfa da pílula", "chrome.rs", "PILL_GLASS_FILL_STRENGTH",
     "appearance.groups.label_alpha", "`.92` da cor cheia"),
    ("borda da aba", "chrome.rs", "TAB_BORDER_WIDTH",
     "appearance.tabs.colors.active_border_width", "**borda 2px**"),
    ("raio da cápsula", "chrome.rs", "WRAPPER_CORNER_RADIUS",
     "appearance.groups.wrapper_corner_radius", None),
    ("altura do rename", "chrome.rs", "RENAME_FIELD_HEIGHT",
     "appearance.tabs.rename.height", None),
    ("largura do rename", "chrome.rs", "RENAME_FIELD_MAX_WIDTH",
     "appearance.tabs.rename.width", None),
    ("raio do quadro do terminal", "paint.rs", "TERMINAL_BOX_CORNER_RADIUS",
     "appearance.terminal_frame.corner_radius", "raio 6"),
    ("resize da janela", "titlebar.rs", "RESIZE_BORDER_PX",
     "appearance.window_controls.resize_border", "**6px** em toda borda"),
]


def constante_rust(fonte: str, nome: str) -> float | None:
    """Valor numérico de um `const NOME: T = v;` ou de um campo `nome: v,`."""
    for padrao in (
        rf"\bconst\s+{nome}\s*:\s*[^=]+=\s*([0-9]+(?:\.[0-9]+)?)",
        rf"\b{nome}\s*:\s*([0-9]+(?:\.[0-9]+)?)\s*,",
    ):
        m = re.search(padrao, fonte)
        if m:
            return float(m.group(1))
    return None


def valor_toml(config: dict, caminho: str) -> float | None:
    atual = config
    for parte in caminho.split("."):
        if not isinstance(atual, dict) or parte not in atual:
            return None
        atual = atual[parte]
    return float(atual) if isinstance(atual, (int, float)) else None


def verificar_valores(config: dict | None) -> None:
    secao("Valores: código, TOML e especificação")
    if config is None:
        erro("pulado — configuração não carregou")
        return
    if not os.path.exists(SPEC):
        erro(f"{SPEC} não encontrado")
        return

    spec = ler(SPEC)
    fontes: dict[str, str] = {}
    for arquivo in {v[1] for v in VALORES}:
        caminho = os.path.join(UI, arquivo)
        if not os.path.exists(caminho):
            erro(f"{caminho} não encontrado")
            return
        fontes[arquivo] = ler(caminho)

    conferidos = 0
    for rotulo, arquivo, nome, chave, trecho in VALORES:
        codigo = constante_rust(fontes[arquivo], nome)
        if codigo is None:
            erro(f"{rotulo}: `{nome}` não existe em {arquivo} — renomeada?")
            continue
        esperado = valor_toml(config, chave)
        if esperado is None:
            erro(f"{rotulo}: chave `{chave}` ausente (ou não numérica) em {CONFIG}")
            continue
        if esperado != codigo:
            erro(
                f"{rotulo}: código {codigo} != TOML {esperado} "
                f"(`{nome}` em {arquivo} vs `{chave}`)"
            )
            continue
        if trecho is not None and trecho not in spec:
            erro(f'{rotulo}: {SPEC} não menciona "{trecho}"')
            continue
        conferidos += 1

    # Duas geometrias derivadas, e são justamente as que já divergiram por
    # cópia: a altura da barra vinha de duas fórmulas em dois lugares
    # (`chrome::bar_height` é a única fonte hoje), e a largura de aba virou
    # fixa, com o teto do rótulo servindo também de piso.
    tb = fontes["tab_bar.rs"]

    def campo(nome: str) -> float | None:
        return constante_rust(tb, nome)

    partes_barra = [campo(n) for n in ("tab_height", "wrapper_padding", "trilha_padding")]
    if all(v is not None for v in partes_barra):
        barra = partes_barra[0] + partes_barra[1] * 2 + partes_barra[2] * 2
        declarada = valor_toml(config, "appearance.tabs.height")
        if declarada != barra:
            erro(
                f"altura da barra: chrome::bar_height dá {barra}, "
                f"`appearance.tabs.height` diz {declarada}"
            )
        else:
            conferidos += 1

    partes_aba = [
        campo(n)
        for n in (
            "padding_left",
            "label_max_width",
            "internal_gap",
            "close_button_size",
            "icon_button_padding_x",
            "padding_right",
        )
    ]
    if all(v is not None for v in partes_aba):
        pl, rotulo_max, gap, fechar, pad_x, pr = partes_aba
        largura = pl + rotulo_max + gap + (fechar + pad_x * 2) + pr
        piso = valor_toml(config, "appearance.tabs.min_width")
        if piso != largura:
            erro(
                f"largura de aba: TabBarStyle::tab_width dá {largura}, "
                f"`appearance.tabs.min_width` diz {piso}"
            )
        else:
            conferidos += 1

    if conferidos == len(VALORES) + 2:
        ok(f"{conferidos} valores batem entre código, {CONFIG} e {SPEC}")


def main() -> int:
    os.chdir(ROOT)
    print(f"Porecatu — verificação de documentação\nraiz: {ROOT}")

    verificar_links()
    config = carregar_config()
    verificar_tokens(config)
    verificar_fases()
    verificar_valores(config)

    print()
    if falhas:
        print(f"FALHOU: {len(falhas)} problema(s).")
        return 1
    print("Tudo certo.")
    return 0


if __name__ == "__main__":
    sys.exit(main())
