@group(0) @binding(0)
var<uniform> globals: vec4<f32>;

@group(1) @binding(0)
var atlas: texture_2d<f32>;
@group(1) @binding(1)
var atlas_sampler: sampler;

struct VertexOutput {
    @builtin(position) position: vec4<f32>,
    @location(0) uv: vec2<f32>,
    @location(1) @interpolate(flat) fg: vec4<f32>,
    @location(2) @interpolate(flat) bg: vec4<f32>,
}

@vertex
fn vs_main(
    @builtin(vertex_index) vertex_index: u32,
    @location(0) rect: vec4<u32>,
    @location(1) uv: vec2<u32>,
    @location(2) fg: vec4<f32>,
    @location(3) bg: vec4<f32>,
) -> VertexOutput {
    let corner = vec2<f32>(f32(vertex_index & 1u), f32(vertex_index >> 1u));
    let offset = corner * vec2<f32>(rect.zw);
    let pixel = vec2<f32>(rect.xy) + offset;

    var out: VertexOutput;
    out.position = vec4<f32>((2.0 * pixel / globals.xy - 1.0) * vec2<f32>(1.0, -1.0), 0.0, 1.0);
    out.uv = (vec2<f32>(uv) + offset) / globals.zw;
    out.fg = fg;
    out.bg = bg;
    return out;
}

@fragment
fn fs_cell(in: VertexOutput) -> @location(0) vec4<f32> {
    let coverage = textureSample(atlas, atlas_sampler, in.uv).r * in.fg.a;
    return vec4<f32>(
        mix(in.bg.rgb, in.fg.rgb, coverage),
        coverage + in.bg.a * (1.0 - coverage),
    );
}

@fragment
fn fs_overlay(in: VertexOutput) -> @location(0) vec4<f32> {
    let coverage = textureSample(atlas, atlas_sampler, in.uv).r * in.fg.a;
    return vec4<f32>(in.fg.rgb, coverage);
}
