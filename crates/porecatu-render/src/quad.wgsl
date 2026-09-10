// SPDX-License-Identifier: GPL-3.0-or-later

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

@fragment
fn fs_main(in: VertexOutput) -> @location(0) vec4<f32> {
    let dist = sdf_rounded_box(in.local_pos, in.half_size, in.radius);
    let aa = max(fwidth(dist) * 0.5, 0.0001);
    let fill_alpha = 1.0 - smoothstep(-aa, aa, dist);

    var color = in.color;
    if in.border_width > 0.0 {
        let border_dist = dist + in.border_width;
        let border_alpha = 1.0 - smoothstep(-aa, aa, border_dist);
        let is_border = clamp(border_alpha - fill_alpha, 0.0, 1.0);
        color = mix(in.color, in.border_color, is_border);
    }

    let alpha = color.a * fill_alpha;
    return vec4<f32>(color.rgb * alpha, alpha);
}
