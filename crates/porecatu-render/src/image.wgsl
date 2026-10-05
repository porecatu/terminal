// SPDX-License-Identifier: GPL-3.0-or-later

// Concatenado depois de `sdf.wgsl`: a máscara é a mesma SDF e a mesma cobertura
// de `quad.wgsl` (`shape_coverage`), não uma cópia (ADR-0061 §3).
//
// Saída **premultiplicada** (`rgb * a, a`) para o blend
// `PREMULTIPLIED_ALPHA_BLENDING` do pipeline -- o par certo (CLAUDE.md, "Blend
// mode do pipeline de quad"). A textura é `Rgba8Unorm`, nunca `Srgb`: os bytes
// do arquivo saem crus na surface, como as cores do design.

struct Uniforms {
    resolution: vec2<f32>,
    _pad: vec2<f32>,
};
@group(0) @binding(0) var<uniform> uniforms: Uniforms;

@group(1) @binding(0) var image_texture: texture_2d<f32>;
@group(1) @binding(1) var clamp_sampler: sampler;
@group(1) @binding(2) var repeat_sampler: sampler;

struct VertexInput {
    @location(0) corner: vec2<f32>,
};

struct InstanceInput {
    @location(1) rect_pos: vec2<f32>,
    @location(2) rect_size: vec2<f32>,
    @location(3) uv_origin: vec2<f32>,
    @location(4) uv_size: vec2<f32>,
    @location(5) mask_pos: vec2<f32>,
    @location(6) mask_size: vec2<f32>,
    @location(7) mask_radius: f32,
    @location(8) alpha: f32,
    @location(9) repeat: f32,
};

struct VertexOutput {
    @builtin(position) clip_position: vec4<f32>,
    @location(0) uv: vec2<f32>,
    @location(1) mask_local: vec2<f32>,
    @location(2) mask_half: vec2<f32>,
    @location(3) mask_radius: f32,
    @location(4) alpha: f32,
    @location(5) repeat: f32,
};

@vertex
fn vs_main(vert: VertexInput, inst: InstanceInput) -> VertexOutput {
    let half_size = inst.rect_size * 0.5;
    let center = inst.rect_pos + half_size;
    let world_pos = center + vert.corner * half_size;

    let ndc_x = (world_pos.x / uniforms.resolution.x) * 2.0 - 1.0;
    let ndc_y = 1.0 - (world_pos.y / uniforms.resolution.y) * 2.0;

    let mask_half = inst.mask_size * 0.5;
    let mask_center = inst.mask_pos + mask_half;

    var out: VertexOutput;
    out.clip_position = vec4<f32>(ndc_x, ndc_y, 0.0, 1.0);
    // (-1,-1) é o canto superior esquerdo, como no quad: uv (0,0) é o texel
    // de cima à esquerda.
    out.uv = inst.uv_origin + (vert.corner * 0.5 + vec2<f32>(0.5, 0.5)) * inst.uv_size;
    out.mask_local = world_pos - mask_center;
    out.mask_half = mask_half;
    out.mask_radius = inst.mask_radius;
    out.alpha = inst.alpha;
    out.repeat = inst.repeat;
    return out;
}

@fragment
fn fs_main(in: VertexOutput) -> @location(0) vec4<f32> {
    // As derivadas saem do fluxo uniforme; `textureSampleGrad` pode então ficar
    // dentro do `if` sem a restrição de derivada implícita.
    let ddx = dpdx(in.uv);
    let ddy = dpdy(in.uv);
    var texel: vec4<f32>;
    if in.repeat > 0.5 {
        texel = textureSampleGrad(image_texture, repeat_sampler, in.uv, ddx, ddy);
    } else {
        texel = textureSampleGrad(image_texture, clamp_sampler, in.uv, ddx, ddy);
    }

    let coverage = shape_coverage(in.mask_local, in.mask_half, in.mask_radius);
    // O alfa do arquivo é reto; aqui ele entra na conta e o rgb é premultiplicado.
    let a = texel.a * in.alpha * coverage;
    return vec4<f32>(texel.rgb * a, a);
}
