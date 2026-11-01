// Color glyphs: premultiplied RGBA atlas, read 1:1 by texel coordinate.
//
// A module of its own, not part of composite.wgsl: both would declare
// `@group(1) @binding(0)`. Texel coordinates because `vs_main` divides by
// `globals.zw`, the size of the 2048x2048 R8 atlas, which would sample the
// wrong place in a color atlas of any other size.
@group(0) @binding(0)
var<uniform> globals: vec4<f32>;

@group(1) @binding(0)
var color_atlas: texture_2d<f32>;

struct VertexOutput {
    @builtin(position) position: vec4<f32>,
    @location(0) uv: vec2<f32>,
    @location(1) @interpolate(flat) bg: vec4<f32>,
}

@vertex
fn vs_color(
    @builtin(vertex_index) vertex_index: u32,
    @location(0) rect: vec4<u32>,
    @location(1) uv: vec2<u32>,
    @location(3) bg: vec4<f32>,
) -> VertexOutput {
    let corner = vec2<f32>(f32(vertex_index & 1u), f32(vertex_index >> 1u));
    let offset = corner * vec2<f32>(rect.zw);
    let pixel = vec2<f32>(rect.xy) + offset;

    var out: VertexOutput;
    out.position = vec4<f32>((2.0 * pixel / globals.xy - 1.0) * vec2<f32>(1.0, -1.0), 0.0, 1.0);
    out.uv = vec2<f32>(uv) + offset;
    out.bg = bg;
    return out;
}

@fragment
fn fs_color(in: VertexOutput) -> @location(0) vec4<f32> {
    let t = textureLoad(color_atlas, vec2<i32>(in.uv), 0);
    return vec4<f32>(t.rgb + in.bg.rgb * (1.0 - t.a), t.a + in.bg.a * (1.0 - t.a));
}
