import bevy_pbr::render::{
    pbr_fragment::pbr_input_from_standard_material,
    pbr_functions::alpha_discard,
};

@if(PREPASS_PIPELINE)
import bevy_pbr::{
    prepass::io::{VertexOutput, FragmentOutput},
    deferred::functions::deferred_output,
};
@else
import bevy_pbr::render::{
    forward_io::{VertexOutput, FragmentOutput},
    pbr_functions::apply_pbr_lighting,
};

struct CrtUniforms {
    effect_intensity: f32,
    time: f32,
    scan_line_intensity: f32,
    chromatic_aberration: f32,
}

@group(constants::MATERIAL_BIND_GROUP) @binding(100)
var<uniform> crt: CrtUniforms;

fn scanline(uv: vec2<f32>) -> f32 {
    let scan_freq = 200.0;
    let scan_pattern = sin(uv.y * scan_freq + crt.time * 2.0);
    return 1.0 - scan_pattern * crt.scan_line_intensity;
}

fn vignette(uv: vec2<f32>) -> f32 {
    let dist = distance(uv, vec2<f32>(0.5, 0.5));
    return smoothstep(0.8, 0.3, dist);
}

@fragment
fn fragment(
    in: VertexOutput,
    @builtin(front_facing) is_front: bool,
) -> FragmentOutput {
    var pbr_input = pbr_input_from_standard_material(in, is_front);

    pbr_input.material.base_color = alpha_discard(
        pbr_input.material.flags,
        pbr_input.material.alpha_cutoff,
        pbr_input.material.base_color
    );

    var out: FragmentOutput;
    @if(PREPASS_PIPELINE) {
        out = deferred_output(in, pbr_input);
    } @else {
        out.color = apply_pbr_lighting(pbr_input);

        if crt.effect_intensity > 0.01 {
            var rgb = out.color.rgb;

            let scan_factor = scanline(in.uv);
            rgb *= scan_factor;

            let vignette_factor = vignette(in.uv);
            rgb *= mix(1.0, vignette_factor, crt.effect_intensity);

            rgb = pow(rgb, vec3<f32>(0.9));

            let shift = crt.chromatic_aberration * crt.effect_intensity;
            rgb.r *= 1.0 + shift;
            rgb.b *= 1.0 - shift;

            out.color = vec4<f32>(rgb, out.color.a);
        }
    }

    return out;
}
