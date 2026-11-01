import bevy_pbr::render::mesh_functions::{get_world_from_local, mesh_position_local_to_clip};

struct VertexInput {
    @builtin(instance_index) instance_index: u32,
    @location(0) position: vec3<f32>,
    @location(1) color: vec4<f32>,
    @location(2) uv: vec2<f32>,
}

struct VertexOutput {
    @builtin(position) clip_position: vec4<f32>,
    @location(0) uv: vec2<f32>,
    @location(1) color: vec4<f32>,
}

@vertex
fn vertex(vertex: VertexInput) -> VertexOutput {
    var out: VertexOutput;

    out.clip_position = mesh_position_local_to_clip(
        get_world_from_local(vertex.instance_index),
        vec4<f32>(vertex.position, 1.0)
    );
    out.uv = vertex.uv;
    out.color = vertex.color;

    return out;
}

struct BlurUniforms {
    effect_intensity: f32,
    time: f32,
    blur_radius: f32,
    blur_samples: f32,
}

@group(constants::MATERIAL_BIND_GROUP) @binding(0)
var<uniform> uniforms: BlurUniforms;

@group(constants::MATERIAL_BIND_GROUP) @binding(1)
var base_color_texture: texture_2d<f32>;

@group(constants::MATERIAL_BIND_GROUP) @binding(2)
var base_color_sampler: sampler;

fn sample_blur(base_texture: texture_2d<f32>, tex_sampler: sampler, uv: vec2<f32>, radius: f32) -> vec3<f32> {
    let texel_size = 1.0 / vec2<f32>(textureDimensions(base_texture));
    var color = vec3<f32>(0.0);
    var weight_sum = 0.0;

    for (var x = -2; x <= 2; x++) {
        for (var y = -2; y <= 2; y++) {
            let offset = vec2<f32>(f32(x), f32(y)) * texel_size * radius;
            let sample_uv = uv + offset;
            let sample_color = textureSample(base_texture, tex_sampler, sample_uv).rgb;
            let weight = 1.0;
            color += sample_color * weight;
            weight_sum += weight;
        }
    }

    return color / weight_sum;
}

fn apply_additive_blur(color: vec3<f32>, uv: vec2<f32>, radius: f32) -> vec3<f32> {
    let blur_strength = radius * 0.2;
    var enhanced = color * (1.0 + blur_strength * 0.5);

    let glow_variation = (sin(uv.x * 8.0) + sin(uv.y * 8.0)) * 0.1 * blur_strength;
    enhanced += vec3<f32>(abs(glow_variation) * 0.3, abs(glow_variation) * 0.2, abs(glow_variation) * 0.1);

    return enhanced;
}

@fragment
fn fragment(in: VertexOutput) -> @location(0) vec4<f32> {
    let blurred = sample_blur(base_color_texture, base_color_sampler, in.uv, uniforms.blur_radius);

    var glow = apply_additive_blur(blurred, in.uv, uniforms.blur_radius);
    glow += vec3<f32>(0.10, 0.06, 0.02);

    let reflection_strength = 0.8;
    let final_color = glow * in.color.rgb * (uniforms.effect_intensity * reflection_strength);
    return vec4<f32>(final_color, 1.0);
}
