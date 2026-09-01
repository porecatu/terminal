// SPDX-License-Identifier: GPL-3.0-or-later

//! Ícones do chrome na face [`FontFace::Icon`][crate::FontFace::Icon]
//! (Lucide, `assets/fonts/Lucide.ttf`, ISC -- compatível com a GPLv3).
//!
//! O arquivo vem do pacote `lucide-static`, que mapeia cada ícone para um
//! codepoint da Área de Uso Privado. O nome da constante é o nome do ícone
//! no catálogo do Lucide (<https://lucide.dev/icons>), em `SCREAMING_CASE`;
//! o comentário guarda o nome original, que é a chave de busca lá.
//!
//! Cada ícone é um [`Icon`] -- o codepoint mais o tamanho do desenho dentro
//! da em. O consumidor é sempre um [`TextRun`][crate::TextRun]: não há
//! primitiva de ícone, o ícone é texto numa face que só tem ícones. Isso
//! mantém `porecatu-render` sem domínio (a lista abaixo é do catálogo da
//! fonte, não de aba nem grupo) e faz o ícone passar pelo mesmo atlas de
//! glyphs, medição e recorte que o resto do texto.
//!
//! A fonte embutida não é subsetada -- 2059 ícones, dos quais este módulo
//! nomeia os poucos em uso. Ao contrário da IBM Plex (ADR-0016), aqui o
//! subsetting seria *permitido* pela ISC; ele não foi feito porque exigiria
//! uma dependência de build só para isso.
//!
//! # `size_px` é a em, não o desenho
//!
//! É a pegadinha desta face, e ela morde de dois jeitos ao mesmo tempo.
//!
//! Toda glyph avança **1 em** e tem o desenho centrado nela, mas o desenho
//! ocupa só parte disso: 0.59 em num `x`, 0.34 em na largura de um
//! `chevron-right`. Pedir `size_px = 10` porque a especificação diz "✕
//! 10px" desenha um ✕ de **5.9 px** -- foi o que aconteceu, e o relato foi
//! "os ícones ficaram muito pequenos".
//!
//! Junto vem o efeito de cor. O traço do Lucide é `2/24` da em: a 10 px ele
//! tem 0.83 px de espessura, o antialiasing o espalha por dois pixels a
//! meia cobertura, e o resultado na tela fica **a meio caminho do fundo** --
//! o relato foi "quase da mesma cor do fundo, quase invisíveis". Não era a
//! cor: `#727a86` é o token da especificação e continua sendo. Era o traço
//! fino demais para render sólido. Dobrar a em dobra o traço, e a cor volta
//! a ser a que o token diz.
//!
//! Por isso [`Icon::ink_width`] existe: um layout que reserve espaço para
//! um ícone precisa da largura do **desenho**, não do avanço de 1 em que
//! ele não preenche.

use crate::primitives::Rect;

/// Onde fica o centro vertical da em de um ícone, contado do topo da
/// [`TextRun`][crate::TextRun] e em múltiplos de `size_px`.
///
/// Não é `0.5`: a face declara ascent = em e descent = 0, então a em vai de
/// `baseline - size_px` até `baseline`; o `cosmic-text` centra o bloco
/// ascent+descent dentro da altura de linha (`size_px * 1.2`, a mesma que o
/// pipeline e o medidor usam), o que põe a baseline em `1.1 * size_px`
/// abaixo do topo. Daí `1.1 - 0.5 = 0.6`. Centrar por `size_px / 2.0`, como
/// se faz com texto, desenha o ícone `0.1 * size_px` baixo demais.
///
/// Vale para o desenho e não só para a em porque o desenho **é** centrado
/// na em nos dois eixos -- as duas coisas são pinadas por teste contra a
/// rasterização real, não estimadas.
const EM_CENTER_FROM_TOP: f32 = 0.6;

/// Um ícone da face: o codepoint e o tamanho do desenho dentro da em, em
/// múltiplos de `size_px`. Os dois valores de desenho são medidos da
/// rasterização e pinados por teste; ver a nota do módulo sobre por que
/// eles não são `1.0`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Icon {
    /// O codepoint, pronto para virar `TextRun::text`.
    pub glyph: &'static str,
    /// Largura do desenho, em múltiplos de `size_px`.
    pub ink_width_em: f32,
    /// Altura do desenho, em múltiplos de `size_px`.
    pub ink_height_em: f32,
}

impl Icon {
    /// Origem de uma [`TextRun`][crate::TextRun] que deixa este ícone
    /// centrado em `rect`, desenhado em `size_px`.
    ///
    /// Não mede texto: toda glyph desta face avança exatamente 1 em (pinado
    /// por teste), e medir no caminho de pintura é a armadilha de
    /// performance registrada no CLAUDE.md.
    pub fn centered_origin(&self, rect: Rect, size_px: f32) -> (f32, f32) {
        (
            rect.x + (rect.width - size_px) / 2.0,
            rect.y + rect.height / 2.0 - EM_CENTER_FROM_TOP * size_px,
        )
    }

    /// Largura do desenho em pixels -- o que um layout deve reservar para o
    /// ícone, em vez do avanço de 1 em que ele não preenche.
    pub fn ink_width(&self, size_px: f32) -> f32 {
        self.ink_width_em * size_px
    }
}

/// `x` -- botão de fechar (aba, aviso).
pub const X: Icon = Icon {
    glyph: "\u{e1b2}",
    ink_width_em: 0.590,
    ink_height_em: 0.580,
};
/// `plus` -- botão de nova aba (global e por grupo).
pub const PLUS: Icon = Icon {
    glyph: "\u{e13d}",
    ink_width_em: 0.675,
    ink_height_em: 0.670,
};
/// `chevron-right` -- caret da pílula de grupo colapsado; overflow à direita.
pub const CHEVRON_RIGHT: Icon = Icon {
    glyph: "\u{e06f}",
    ink_width_em: 0.340,
    ink_height_em: 0.590,
};
/// `chevron-down` -- caret da pílula de grupo expandido.
pub const CHEVRON_DOWN: Icon = Icon {
    glyph: "\u{e06d}",
    ink_width_em: 0.590,
    ink_height_em: 0.340,
};
/// `chevron-left` -- overflow à esquerda.
pub const CHEVRON_LEFT: Icon = Icon {
    glyph: "\u{e06e}",
    ink_width_em: 0.340,
    ink_height_em: 0.590,
};

/// Todos os ícones nomeados, com o nome do catálogo -- é o que os testes
/// varrem para pegar glyph ausente ou tamanho de desenho desatualizado.
pub const ALL: [(&str, Icon); 9] = [
    ("x", X),
    ("plus", PLUS),
    ("chevron-right", CHEVRON_RIGHT),
    ("chevron-down", CHEVRON_DOWN),
    ("chevron-left", CHEVRON_LEFT),
    ("settings", SETTINGS),
    ("minus", MINUS),
    ("square", MAXIMIZE),
    ("copy", RESTORE),
];

/// `minus` -- botão de minimizar a janela (ADR-0027).
pub const MINUS: Icon = Icon {
    glyph: "\u{e11c}",
    ink_width_em: 0.675,
    ink_height_em: 0.09,
};
/// `square` -- botão de maximizar a janela, estado não-maximizado
/// (ADR-0027).
pub const MAXIMIZE: Icon = Icon {
    glyph: "\u{e167}",
    ink_width_em: 0.83,
    ink_height_em: 0.84,
};
/// `copy` -- botão de restaurar a janela, estado maximizado (ADR-0027).
/// Lucide não tem um ícone dedicado "restore window"; dois quadrados
/// sobrepostos é a aproximação que outras suítes de ícone usam para o
/// mesmo conceito.
pub const RESTORE: Icon = Icon {
    glyph: "\u{e09e}",
    ink_width_em: 0.92,
    ink_height_em: 0.92,
};

/// A maior largura de desenho entre os dois carets da pílula de grupo. Um
/// layout que reserve espaço para "o caret" precisa deste valor, e não do
/// de um dos dois: o `chevron-down` é quase o dobro da largura do
/// `chevron-right`, e reservar pelo menor faria a pílula mudar de largura
/// ao colapsar -- justo no gesto que já está animando.
pub const WIDEST_CARET_INK_EM: f32 = CHEVRON_DOWN.ink_width_em;

/// `settings` -- botão de configurações da zona fixa à direita da barra.
pub const SETTINGS: Icon = Icon {
    glyph: "\u{e154}",
    ink_width_em: 0.840,
    ink_height_em: 0.920,
};
