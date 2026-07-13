// Wave Distortion Shader
// Vertex shader that deforms the mesh with:
// - Sine wave displacement
// - Ripple effects from center
// - Animated movement

#import bevy_pbr::{
    mesh_functions,
    forward_io::{Vertex, VertexOutput},
    view_transformations::position_world_to_clip,
    mesh_view_bindings::view,
}

@group(2) @binding(0) var terminal_texture: texture_2d<f32>;
@group(2) @binding(1) var terminal_sampler: sampler;
@group(2) @binding(2) var<uniform> wave_amplitude: f32;
@group(2) @binding(3) var<uniform> wave_frequency: f32;
@group(2) @binding(4) var<uniform> time: f32;
@group(2) @binding(5) var<uniform> effect_enabled: f32;

const PI: f32 = 3.14159265359;

@vertex
fn vertex(vertex: Vertex) -> VertexOutput {
    var out: VertexOutput;
    var position = vertex.position;

    if effect_enabled > 0.5 {
        // Normalized coordinates (-1 to 1)
        let x = position.x / 2.0; // Assuming plane width is 4.0
        let y = position.y / 1.5; // Assuming plane height is 3.0

        // Sine wave along X axis
        let wave_x = sin(x * wave_frequency + time * 2.0) * wave_amplitude;

        // Ripple from center
        let dist = length(vec2<f32>(x, y));
        let ripple = sin(dist * 10.0 - time * 3.0) * wave_amplitude * 0.3;

        // Combine effects
        position.z += wave_x + ripple;

        // Add slight Y displacement for more dynamic effect
        position.y += sin(x * wave_frequency * 0.5 + time) * wave_amplitude * 0.2;
    }

    // Transform to world space
    var world_from_local = mesh_functions::get_world_from_local(vertex.instance_index);
    let world_position = mesh_functions::mesh_position_local_to_world(world_from_local, vec4<f32>(position, 1.0));

    out.position = position_world_to_clip(world_position.xyz);
    out.world_position = world_position;

    // Pass through normals and UVs
    out.world_normal = mesh_functions::mesh_normal_local_to_world(
        vertex.normal,
        vertex.instance_index
    );

    out.uv = vertex.uv;

    #ifdef VERTEX_TANGENTS
    out.world_tangent = mesh_functions::mesh_tangent_local_to_world(
        world_from_local,
        vertex.tangent
    );
    #endif

    return out;
}

@fragment
fn fragment(
    mesh: VertexOutput,
) -> @location(0) vec4<f32> {
    // Simple texture sampling
    let color = textureSample(terminal_texture, terminal_sampler, mesh.uv);
    return color;
}
