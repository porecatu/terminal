// SPDX-License-Identifier: GPL-3.0-or-later

// Um passe de gaussiana separável (ADR-0068 §3): o mesmo shader faz o horizontal
// e o vertical, com `step` apontando para um eixo ou para o outro. Os pesos vêm
// já normalizados da CPU (`blur.rs`, `weights`): `w0 + 2 * soma(w1..) = 1`.
//
// Premultiplicado de ponta a ponta: a fonte é desenhada com o blend
// premultiplicado e a média de cores premultiplicadas é a cor premultiplicada da
// média, então o passe só soma amostras -- nada de converter alfa. A textura é
// `Unorm`, nunca `Srgb` (ADR-0061 §3): os bytes passam crus.
//
// `textureSampleLevel`, não `textureSample`: o laço tem limite lido de uniform, e
// derivadas implícitas exigiriam fluxo uniforme que o validador não consegue
// provar. Sem mips aqui, o nível é sempre 0.

struct Params {
    // Passo de um texel da textura reduzida, já no eixo do passe: (1/w, 0) ou
    // (0, 1/h).
    step: vec2<f32>,
    // Amostras de cada lado do centro (no máximo 15).
    taps: u32,
    // Distância, em texels, entre uma amostra e a seguinte.
    stride: f32,
    // `weights[i]` é o peso da amostra `i` (0 é o centro), 16 floats em 4 vec4
    // para o alinhamento do uniform.
    weights: array<vec4<f32>, 4>,
};

@group(0) @binding(0) var<uniform> params: Params;
@group(0) @binding(1) var source_texture: texture_2d<f32>;
@group(0) @binding(2) var source_sampler: sampler;

struct VertexOutput {
    @builtin(position) clip_position: vec4<f32>,
    @location(0) uv: vec2<f32>,
};

// Triângulo que cobre o alvo inteiro, sem buffer de vértices (o mesmo de
// `blit.wgsl`, com o mesmo `uv` com o eixo Y invertido).
@vertex
fn vs_main(@builtin(vertex_index) index: u32) -> VertexOutput {
    let uv = vec2<f32>(f32((index << 1u) & 2u), f32(index & 2u));
    var out: VertexOutput;
    out.clip_position = vec4<f32>(uv * 2.0 - 1.0, 0.0, 1.0);
    out.uv = vec2<f32>(uv.x, 1.0 - uv.y);
    return out;
}

fn weight(i: u32) -> f32 {
    return params.weights[i / 4u][i % 4u];
}

@fragment
fn fs_main(in: VertexOutput) -> @location(0) vec4<f32> {
    // O sampler é de endereçamento `ClampToEdge`: a borda da janela repete o
    // texel de fora, e a imagem não escurece rumo a ela.
    var acc = textureSampleLevel(source_texture, source_sampler, in.uv, 0.0) * weight(0u);
    for (var i = 1u; i <= params.taps; i = i + 1u) {
        let offset = params.step * (f32(i) * params.stride);
        let both = textureSampleLevel(source_texture, source_sampler, in.uv + offset, 0.0)
            + textureSampleLevel(source_texture, source_sampler, in.uv - offset, 0.0);
        acc = acc + both * weight(i);
    }
    return acc;
}
