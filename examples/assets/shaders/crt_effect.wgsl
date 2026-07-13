// CRT Effect Shader
// Simulates a cathode ray tube (CRT) monitor with:
// - Scan lines
// - Screen curvature
// - Color aberration (chromatic aberration)
// - Vignette

#import bevy_pbr::forward_io::VertexOutput

struct CrtMaterial {
    effect_intensity: f32,
    time: f32,
};

@group(2) @binding(100) var<uniform> material: CrtMaterial;
@group(2) @binding(101) var terminal_texture: texture_2d<f32>;
@group(2) @binding(102) var terminal_sampler: sampler;

// Screen curvature function
fn curve_uv(uv: vec2<f32>) -> vec2<f32> {
    let curvature = 0.15 * material.effect_intensity;
    let uv_centered = uv * 2.0 - 1.0;
    let offset = uv_centered.yx * uv_centered.yx;
    let curved = uv_centered + uv_centered * offset * curvature;
    return curved * 0.5 + 0.5;
}

// Vignette effect
fn vignette(uv: vec2<f32>) -> f32 {
    let dist = distance(uv, vec2<f32>(0.5, 0.5));
    return smoothstep(0.8, 0.3, dist);
}

// Scan line effect
fn scanline(uv: vec2<f32>) -> f32 {
    let scan_freq = 200.0;
    let scan_intensity = 0.1 * material.effect_intensity;
    return 1.0 - sin(uv.y * scan_freq + material.time * 2.0) * scan_intensity;
}

@fragment
fn fragment(
    mesh: VertexOutput,
) -> @location(0) vec4<f32> {
    var uv = mesh.uv;

    // Apply screen curvature
    let curved_uv = curve_uv(uv);

    // Discard pixels outside screen bounds (black bars)
    if curved_uv.x < 0.0 || curved_uv.x > 1.0 || curved_uv.y < 0.0 || curved_uv.y > 1.0 {
        return vec4<f32>(0.0, 0.0, 0.0, 1.0);
    }

    // Color aberration (chromatic aberration)
    let aberration = 0.002 * material.effect_intensity;
    let r = textureSample(terminal_texture, terminal_sampler, curved_uv - vec2<f32>(aberration, 0.0)).r;
    let g = textureSample(terminal_texture, terminal_sampler, curved_uv).g;
    let b = textureSample(terminal_texture, terminal_sampler, curved_uv + vec2<f32>(aberration, 0.0)).b;
    var color = vec3<f32>(r, g, b);

    // Apply scan lines
    color *= scanline(curved_uv);

    // Apply vignette
    color *= mix(1.0, vignette(curved_uv), material.effect_intensity);

    // Slight phosphor glow
    color = pow(color, vec3<f32>(0.9));

    return vec4<f32>(color, 1.0);
}

