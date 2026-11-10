// SPDX-License-Identifier: GPL-3.0-or-later

// Concatenado depois de `sdf.wgsl` (sdf_segment, edge_aa, edge_coverage): a
// cobertura de bordas é a mesma de `quad.wgsl`, não uma cópia (ADR-0094).
//
// Uma instância por segmento, em pixels físicos. `kind = 0` é um segmento de
// polilinha (cápsula por SDF); `kind = 1` é uma faixa vertical de área, sob o
// segmento `p0`-`p1`, até `half_width` (que ali é o `y` da base). A cor chega
// premultiplicada.

struct Uniforms {
    resolution: vec2<f32>,
    _pad: vec2<f32>,
};
@group(0) @binding(0) var<uniform> uniforms: Uniforms;

struct VertexInput {
    @location(0) corner: vec2<f32>,
};

struct InstanceInput {
    @location(1) p0: vec2<f32>,
    @location(2) p1: vec2<f32>,
    @location(3) half_width: f32,
    @location(4) kind: u32,
    @location(5) _pad: vec2<f32>,
    @location(6) color: vec4<f32>,
};

struct VertexOutput {
    @builtin(position) clip_position: vec4<f32>,
    @location(0) @interpolate(flat) p0: vec2<f32>,
    @location(1) @interpolate(flat) p1: vec2<f32>,
    @location(2) @interpolate(flat) half_width: f32,
    @location(3) @interpolate(flat) kind: u32,
    @location(4) @interpolate(flat) color: vec4<f32>,
};

@vertex
fn vs_main(vert: VertexInput, inst: InstanceInput) -> VertexOutput {
    var world_pos: vec2<f32>;
    if inst.kind == 0u {
        // Retângulo orientado em volta do segmento, estendido por
        // `half_width + 1 px` nas pontas e nos lados: o que a rampa de
        // antisserrilhado ainda toca cabe dentro dele.
        let ext = inst.half_width + 1.0;
        let d = inst.p1 - inst.p0;
        let len = length(d);
        var dir = vec2<f32>(1.0, 0.0);
        if len > 0.0 {
            dir = d / len;
        }
        let normal = vec2<f32>(-dir.y, dir.x);
        let u = (vert.corner.x + 1.0) * 0.5;
        let along = mix(-ext, len + ext, u);
        let across = vert.corner.y * ext;
        world_pos = inst.p0 + dir * along + normal * across;
    } else {
        // Retângulo alinhado aos eixos [x0, x1] x [min(y0, y1) - 1, base]. Os
        // `x` vêm direto das instâncias (sem `mix`): faixas vizinhas
        // compartilham o mesmo `f32` e a regra top-left da rasterização
        // entrega cada pixel a uma só delas, sem costura.
        let top = min(inst.p0.y, inst.p1.y) - 1.0;
        let bottom = max(inst.half_width, top);
        let x = select(inst.p1.x, inst.p0.x, vert.corner.x < 0.0);
        let y = select(bottom, top, vert.corner.y < 0.0);
        world_pos = vec2<f32>(x, y);
    }

    let ndc_x = (world_pos.x / uniforms.resolution.x) * 2.0 - 1.0;
    let ndc_y = 1.0 - (world_pos.y / uniforms.resolution.y) * 2.0;

    var out: VertexOutput;
    out.clip_position = vec4<f32>(ndc_x, ndc_y, 0.0, 1.0);
    out.p0 = inst.p0;
    out.p1 = inst.p1;
    out.half_width = inst.half_width;
    out.kind = inst.kind;
    out.color = inst.color;
    return out;
}

@fragment
fn fs_main(in: VertexOutput) -> @location(0) vec4<f32> {
    // Centro do pixel, em pixels físicos.
    let pos = in.clip_position.xy;

    // Polilinha: cobertura da cápsula, com a rampa das bordas arredondadas.
    // Calculada sem ramo, porque `fwidth` exige fluxo uniforme.
    let dist = sdf_segment(pos, in.p0, in.p1) - in.half_width;
    let line_cov = edge_coverage(dist, edge_aa(dist));

    // Área: `y` da linha no centro do pixel; cheio abaixo dela, rampa de um
    // pixel em cima. Sem antisserrilhado lateral.
    let span = in.p1.x - in.p0.x;
    let t = clamp((pos.x - in.p0.x) / max(span, 0.0001), 0.0, 1.0);
    let y_line = mix(in.p0.y, in.p1.y, t);
    let area_cov = clamp(pos.y - y_line + 0.5, 0.0, 1.0);

    let cov = select(line_cov, area_cov, in.kind == 1u);
    return in.color * cov;
}
