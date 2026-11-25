#!/usr/bin/env python3
# SPDX-License-Identifier: GPL-3.0-or-later
"""
Gera as faces embutidas do Porecatu a partir dos TTF originais da Iosevka.

    python scripts/subset-fonts.py <dir-com-os-ttf-originais>

Terminal e chrome usam a mesma família, Iosevka Fixed (ADR-0026, que
unificou o que o ADR-0025 tinha dividido entre Fixed e Aile). As faces da
Iosevka vêm com ~8.7 MB cada, porque o pacote traz todos os conjuntos
estilísticos alternativos. Este script recorta cada face para os blocos
que o projeto realmente desenha, o que as leva à casa de 200 KB.

O recorte é **permitido**: a Iosevka é SIL OFL 1.1 **sem** cláusula de
Reserved Font Name (ao contrário da IBM Plex, que a proibia — ver
ADR-0025). O nome da família é preservado de propósito, para que
`family = "Iosevka Fixed"` na config continue significando o que diz.

O que fica de fora sai pela cadeia de fallback do sistema (ADR-0016):
emoji, CJK, e todo o resto do Unicode. É a mesma divisão de sempre — o
binário garante o que o design promete, o sistema cobre o resto.

Dentro das faixas, dois grupos saem do recorte (ADR-0117): todo caractere
de largura dupla na grade (East Asian Width W ou F, calculado aqui, não
listado) e os de `EXCLUIDOS`, que a variante Fixed desenha pequenos
demais. Os dois passam a vir do fallback, que o ADR-0116 encaixa na
célula pela tinta.

Requer `fonttools`. Rodado à mão quando a versão da Iosevka sobe, não no
build: o resultado é versionado em `assets/fonts/`.
"""

from __future__ import annotations

import os
import subprocess
import sys
import tempfile
import unicodedata

# Blocos que o Porecatu desenha por conta própria. Cada faixa está aqui
# por um motivo verificável, não por precaução.
FAIXAS = [
    ("0000-007F", "ASCII"),
    ("0080-00FF", "Latin-1 — acentuação do português"),
    ("0100-017F", "Latin Extended-A"),
    ("0180-024F", "Latin Extended-B"),
    ("0250-02FF", "IPA e modificadores — saída de shell os usa"),
    ("0300-036F", "diacríticos combinantes"),
    ("0370-03FF", "grego — símbolo matemático em saída de programa"),
    ("0400-04FF", "cirílico"),
    ("2000-206F", "pontuação geral — reticências do truncamento (RF-1.10)"),
    ("2070-209F", "sobrescrito e subscrito"),
    ("20A0-20BF", "símbolos de moeda"),
    ("2100-214F", "letras tipo símbolo"),
    ("2190-21FF", "setas"),
    ("2200-22FF", "operadores matemáticos"),
    ("2300-23FF", "técnicos diversos — controles de mídia em TUI"),
    ("2460-24FF", "alfanuméricos cercados"),
    ("2500-257F", "box drawing — moldura de btop, Claude Code, vim"),
    ("2580-259F", "elementos de bloco — barras de progresso"),
    ("25A0-25FF", "formas geométricas — marcadores de TUI"),
    ("2600-26FF", "símbolos diversos"),
    ("2700-27BF", "dingbats — o ✽ do spinner do Claude Code"),
    ("2800-28FF", "braille — os gráficos do btop"),
    ("2900-297F", "setas suplementares"),
    ("2B00-2BFF", "símbolos e setas diversos"),
    ("E0A0-E0D7", "powerline — prompts de shell"),
    ("FE00-FE0F", "seletores de variação"),
    ("FFFD", "caractere de substituição"),
]

# Tirados do recorte mesmo dentro de uma faixa acima (ADR-0117). A Fixed
# espreme em meia em o que a Iosevka normal desenha numa em, e estes saem
# visivelmente menores que o texto ao lado. Medidas da Fixed 34.8.1, em
# fração da em; a altura de versal é 0.735.
EXCLUIDOS = [
    ("26A0", "⚠ — triângulo estreito, 0.44 de largura; o aviso do Claude Code"),
    ("23F4-23F7", "⏴⏵⏶⏷ — triângulos médios de 0.22 × 0.44, 60% da versal; o ⏵⏵ do Claude Code"),
]

FACES = [
    "IosevkaFixed-Regular.ttf",
    "IosevkaFixed-Medium.ttf",
]

DESTINO = os.path.join(os.path.dirname(os.path.dirname(os.path.abspath(__file__))), "assets", "fonts")


def expandir(faixa: str) -> range:
    inicio, _, fim = faixa.partition("-")
    return range(int(inicio, 16), int(fim or inicio, 16) + 1)


def largura_dupla(codepoint: int) -> bool:
    """A grade reserva duas células para ele, e a Fixed desenha uma.

    É a mesma propriedade que o `unicode-width` do `alacritty_terminal`
    usa para marcar a célula como larga. Depende da versão do Unicode do
    Python que roda o script; a 3.12 traz a 15.0.
    """
    return unicodedata.east_asian_width(chr(codepoint)) in ("W", "F")


def codepoints() -> list[int]:
    manter: set[int] = set()
    for faixa, _ in FAIXAS:
        manter.update(expandir(faixa))
    excluir = {cp for faixa, _ in EXCLUIDOS for cp in expandir(faixa)}
    excluir.update(cp for cp in manter if largura_dupla(cp))
    return sorted(manter - excluir)


def main() -> int:
    if len(sys.argv) != 2:
        print(__doc__)
        return 2
    origem = sys.argv[1]
    print(f"Unicode {unicodedata.unidata_version}")

    # Em arquivo, não em `--unicodes=`: a lista explícita, com as exclusões,
    # passa do limite de linha de comando do Windows.
    with tempfile.NamedTemporaryFile("w", suffix=".txt", delete=False, encoding="ascii") as arquivo:
        arquivo.write("\n".join(f"{cp:04X}" for cp in codepoints()))
        lista = arquivo.name

    try:
        total_antes = total_depois = 0
        for face in FACES:
            entrada = os.path.join(origem, face)
            if not os.path.isfile(entrada):
                print(f"ERRO: não achei {entrada}")
                return 1
            saida = os.path.join(DESTINO, face)
            antes = os.path.getsize(entrada)
            subprocess.run(
                [
                    sys.executable,
                    "-m",
                    "fontTools.subset",
                    entrada,
                    f"--unicodes-file={lista}",
                    f"--output-file={saida}",
                    # Sem os conjuntos estilísticos alternativos, que são o
                    # grosso do peso e que o projeto não expõe.
                    "--layout-features=ccmp,locl,mark,mkmk",
                    "--no-hinting",
                    "--desubroutinize",
                    # O nome da família precisa sobreviver: a config e a
                    # especificação visual se referem a ele.
                    "--name-IDs=*",
                    "--glyph-names",
                    "--drop-tables+=DSIG",
                ],
                check=True,
            )
            depois = os.path.getsize(saida)
            total_antes += antes
            total_depois += depois
            print(f"  {face}: {antes // 1024} KB -> {depois // 1024} KB")
    finally:
        os.unlink(lista)

    print(f"\ntotal: {total_antes // 1024 // 1024} MB -> {total_depois // 1024} KB")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
