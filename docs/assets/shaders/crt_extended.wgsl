// CRT Extended Material Shader
// Custom fragment shader for CRT post-processing effects
// Extends StandardMaterial safely using MaterialExtension pattern

#import bevy_pbr::{
    pbr_fragment::pbr_input_from_standard_material,
    pbr_functions::{alpha_discard, apply_pbr_lighting},
    forward_io::VertexOutput,
}

#ifdef PREPASS_PIPELINE
#import bevy_pbr::prepass_io::FragmentOutput
#else
#import bevy_pbr::forward_io::FragmentOutput
#endif

// CRT uniforms struct (matches Rust CrtUniforms exactly)
struct CrtUniforms {
    effect_intensity: f32,
    time: f32,
    scan_line_intensity: f32,
    chromatic_aberration: f32,
}

// CRITICAL: Binding 100 in MATERIAL_BIND_GROUP (Group 3)
// This matches the Rust #[uniform(100)] attribute
// StandardMaterial uses bindings 0-30, so 100 is safely above that range
@group(#{MATERIAL_BIND_GROUP}) @binding(100)
var<uniform> crt: CrtUniforms;

// Animated scan lines effect
fn scanline(uv: vec2<f32>) -> f32 {
    let scan_freq = 200.0;
    let scan_pattern = sin(uv.y * scan_freq + crt.time * 2.0);
    return 1.0 - scan_pattern * crt.scan_line_intensity;
}

// Vignette effect (darkens screen edges)
fn vignette(uv: vec2<f32>) -> f32 {
    let dist = distance(uv, vec2<f32>(0.5, 0.5));
    return smoothstep(0.8, 0.3, dist);
}

@fragment
fn fragment(
    in: VertexOutput,
    @builtin(front_facing) is_front: bool,
) -> FragmentOutput {
    // Get PBR input from StandardMaterial
    // This automatically:
    // - Samples base_color_texture (our terminal texture) at bindings 1-2
    // - Applies StandardMaterial properties
    // - Returns a PbrInput struct with the sampled color
    var pbr_input = pbr_input_from_standard_material(in, is_front);

    // Apply alpha discard (standard PBR operation)
    pbr_input.material.base_color = alpha_discard(
        pbr_input.material,
        pbr_input.material.base_color
    );

    // Apply PBR lighting and generate base output
    #ifdef PREPASS_PIPELINE
        let out = deferred_output(in, pbr_input);
    #else
        var out: FragmentOutput;

        // Apply PBR lighting (includes our terminal texture from StandardMaterial)
        out.color = apply_pbr_lighting(pbr_input);

        // Phases 2-4: Apply CRT effects
        if crt.effect_intensity > 0.01 {
            // Extract RGB to modify (WGSL doesn't allow assignment to swizzles)
            var rgb = out.color.rgb;

            // Phase 2: Scan lines
            let scan_factor = scanline(in.uv);
            rgb *= scan_factor;

            // Phase 3: Vignette
            let vignette_factor = vignette(in.uv);
            rgb *= mix(1.0, vignette_factor, crt.effect_intensity);

            // Phase 4: Phosphor glow + color shift
            // Phosphor glow (subtle gamma adjustment)
            rgb = pow(rgb, vec3<f32>(0.9));

            // Subtle color shift (approximation of chromatic aberration)
            let shift = crt.chromatic_aberration * crt.effect_intensity;
            rgb.r *= 1.0 + shift;
            rgb.b *= 1.0 - shift;

            // Reconstruct color with modified RGB and original alpha
            out.color = vec4<f32>(rgb, out.color.a);
        }
    #endif

    return out;
}
