// SPDX-License-Identifier: GPL-3.0-or-later

// Forma e cobertura de retângulo arredondado, compartilhadas por `quad.wgsl` e
// `image.wgsl`. O Rust concatena este arquivo na frente de cada um deles
// (`concat!(include_str!(..))`, WGSL não tem include): a fórmula mora aqui uma
// vez só, porque fórmula de geometria copiada diverge quando alguém mexe numa
// cópia (CLAUDE.md, F3). Tudo em pixels físicos, `p`/`local_pos` relativos ao
// centro do retângulo.

// SDF de caixa reta (distância de Chebyshev). Usada quando `radius <= 0`:
// a formula de retangulo arredondado abaixo, na regiao de canto (q.x > 0 e
// q.y > 0 ao mesmo tempo), cai em `length(max(q,0))` -- distancia
// euclidiana ate o ponto do canto, que arredonda visivelmente o pixel do
// canto mesmo com `radius = 0.0`. `max(q.x, q.y)` corta reto ali.
fn sdf_box(p: vec2<f32>, half_size: vec2<f32>) -> f32 {
    let q = abs(p) - half_size;
    return max(q.x, q.y);
}

// SDF de retângulo arredondado. `p` relativo ao centro; negativo dentro,
// positivo fora, zero na borda. Formula padrao (Inigo Quilez).
fn sdf_rounded_box(p: vec2<f32>, half_size: vec2<f32>, radius: f32) -> f32 {
    if radius <= 0.0 {
        return sdf_box(p, half_size);
    }
    let q = abs(p) - half_size + vec2<f32>(radius, radius);
    return length(max(q, vec2<f32>(0.0, 0.0))) + min(max(q.x, q.y), 0.0) - radius;
}

// Cobertura de caixa reta por eixo, separavel. `max(q.x, q.y)` (sdf_box) tem
// uma quina de gradiente na diagonal do canto (onde q.x ~= q.y, os dois
// positivos); `fwidth` sobre essa quina mistura a derivada dos dois lados e
// alarga o antialiasing so ali, arredondando visivelmente o canto mesmo com
// a SDF reta. Calculando a cobertura de cada eixo com seu proprio fwidth (que
// nunca depende do outro eixo) e multiplicando, a quina desaparece por
// construcao -- e um retangulo com bordas ja alinhadas ao pixel fisico
// (snap_rect_to_physical_pixels) sai 0/1 puro em todo canto, sem faixa cinza.
fn box_coverage(local_pos: vec2<f32>, half_size: vec2<f32>) -> f32 {
    let edge = half_size - abs(local_pos);
    let aa = max(fwidth(local_pos), vec2<f32>(0.0001, 0.0001));
    let cov = clamp(edge / aa + vec2<f32>(0.5, 0.5), vec2<f32>(0.0, 0.0), vec2<f32>(1.0, 1.0));
    return cov.x * cov.y;
}

// Meia largura da faixa de antialiasing de um contorno arredondado: a
// derivada da distância, que é ~1 pixel.
fn edge_aa(dist: f32) -> f32 {
    return max(fwidth(dist) * 0.5, 0.0001);
}

// Cobertura (1 dentro, 0 fora) de uma distância com sinal, suavizada em `aa`.
fn edge_coverage(dist: f32, aa: f32) -> f32 {
    return 1.0 - smoothstep(-aa, aa, dist);
}

// Cobertura do preenchimento de um retângulo arredondado -- a forma inteira, o
// que `quad.wgsl` pinta como fundo e `image.wgsl` usa de máscara. `radius <= 0`
// segue o ramo de caixa por eixo (`box_coverage`), pela mesma razão do canto
// de bloco.
fn shape_coverage(local_pos: vec2<f32>, half_size: vec2<f32>, radius: f32) -> f32 {
    if radius <= 0.0 {
        return box_coverage(local_pos, half_size);
    }
    let dist = sdf_rounded_box(local_pos, half_size, radius);
    return edge_coverage(dist, edge_aa(dist));
}
