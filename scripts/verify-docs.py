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


def main() -> int:
    os.chdir(ROOT)
    print(f"Porecatu — verificação de documentação\nraiz: {ROOT}")

    verificar_links()
    config = carregar_config()
    verificar_tokens(config)
    verificar_fases()

    print()
    if falhas:
        print(f"FALHOU: {len(falhas)} problema(s).")
        return 1
    print("Tudo certo.")
    return 0


if __name__ == "__main__":
    sys.exit(main())
