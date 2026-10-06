// SPDX-License-Identifier: GPL-3.0-or-later

// Concatenado depois de `sdf.wgsl` (sdf_box, sdf_rounded_box, box_coverage,
// edge_aa, edge_coverage, shape_coverage): a SDF e a cobertura são as mesmas
// de `image.wgsl`, não uma cópia.

struct Uniforms {
    resolution: vec2<f32>,
    _pad: vec2<f32>,
};
@group(0) @binding(0) var<uniform> uniforms: Uniforms;

struct VertexInput {
    @location(0) corner: vec2<f32>,
};

struct InstanceInput {
    @location(1) rect_pos: vec2<f32>,
    @location(2) rect_size: vec2<f32>,
    @location(3) color: vec4<f32>,
    @location(4) radius: f32,
    @location(5) border_width: f32,
    @location(6) _pad: vec2<f32>,
    @location(7) border_color: vec4<f32>,
};

struct VertexOutput {
    @builtin(position) clip_position: vec4<f32>,
    @location(0) local_pos: vec2<f32>,
    @location(1) half_size: vec2<f32>,
    @location(2) color: vec4<f32>,
    @location(3) radius: f32,
    @location(4) border_width: f32,
    @location(5) border_color: vec4<f32>,
};

@vertex
fn vs_main(vert: VertexInput, inst: InstanceInput) -> VertexOutput {
    let half_size = inst.rect_size * 0.5;
    let center = inst.rect_pos + half_size;
    let world_pos = center + vert.corner * half_size;

    let ndc_x = (world_pos.x / uniforms.resolution.x) * 2.0 - 1.0;
    let ndc_y = 1.0 - (world_pos.y / uniforms.resolution.y) * 2.0;

    var out: VertexOutput;
    out.clip_position = vec4<f32>(ndc_x, ndc_y, 0.0, 1.0);
    out.local_pos = vert.corner * half_size;
    out.half_size = half_size;
    out.color = inst.color;
    out.radius = inst.radius;
    out.border_width = inst.border_width;
    out.border_color = inst.border_color;
    return out;
}

@fragment
fn fs_main(in: VertexOutput) -> @location(0) vec4<f32> {
    var fill_alpha: f32;
    var border_frac = 0.0;

    if in.radius <= 0.0 {
        fill_alpha = box_coverage(in.local_pos, in.half_size);
        if in.border_width > 0.0 {
            let outer_cov = box_coverage(in.local_pos, in.half_size + vec2<f32>(in.border_width, in.border_width));
            border_frac = clamp(outer_cov - fill_alpha, 0.0, 1.0);
        }
    } else {
        let dist = sdf_rounded_box(in.local_pos, in.half_size, in.radius);
        let aa = edge_aa(dist);
        fill_alpha = edge_coverage(dist, aa);
        if in.border_width > 0.0 {
            // A borda é o anel de `border_width` por dentro do contorno: o que
            // está dentro do retângulo e fora do mesmo retângulo encolhido.
            // (`border_alpha - fill_alpha` dava sempre <= 0 -- a área
            // encolhida é a menor --, então a borda nunca era pintada.)
            let inner_alpha = edge_coverage(dist + in.border_width, aa);
            // Premultiplicado: miolo na cor de preenchimento, anel na cor da
            // borda **sobre** o preenchimento. Borda transparente deixa o
            // preenchimento como está, em vez de abrir um anel vazio.
            let fill_p = vec4<f32>(in.color.rgb * in.color.a, in.color.a);
            let border_p = vec4<f32>(in.border_color.rgb * in.border_color.a, in.border_color.a);
            let ring_p = border_p + fill_p * (1.0 - in.border_color.a);
            let ring_cov = clamp(fill_alpha - inner_alpha, 0.0, 1.0);
            return fill_p * clamp(inner_alpha, 0.0, 1.0) + ring_p * ring_cov;
        }
    }

    var color = in.color;
    if in.border_width > 0.0 {
        color = mix(in.color, in.border_color, border_frac);
    }

    let alpha = color.a * fill_alpha;
    return vec4<f32>(color.rgb * alpha, alpha);
}

// Primeira metade de um `Primitive::Backdrop` arredondado (ver `QuadMode` em
// `quad.rs`): só a cobertura da forma, no alfa, com cor zero. O pipeline que a
// usa tem `dst * (1 - alfa)` como blend: apaga o destino na proporção em que a
// forma o cobre -- tudo no miolo, nada fora do raio, e a faixa antialiasada
// só em parte. `BlendState::REPLACE` apagava os pixels de cobertura zero dos
// cantos de fora do raio e a faixa inteira, e o que estava atrás da janela
// aparecia ali.
@fragment
fn fs_erase(in: VertexOutput) -> @location(0) vec4<f32> {
    return vec4<f32>(0.0, 0.0, 0.0, shape_coverage(in.local_pos, in.half_size, in.radius));
}
