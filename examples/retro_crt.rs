// ExtendedMaterial CRT Example - Custom Fragment Shader, Mesh3d with CRT Effects
//
// Demonstrates using ExtendedMaterial to extend StandardMaterial with custom
// fragment shader uniforms for CRT post-processing effects (scan lines, vignette).
//
// This approach avoids binding conflicts by:
// - Using StandardMaterial for terminal texture (bindings 0-30)
// - Extending with custom uniforms at binding 100
// - Letting Bevy's PBR system handle texture sampling
//
// Press SPACE to toggle CRT effects
// Press LEFT/RIGHT (or click the tab bar) to switch tabs on the CRT screen
// Press ESC to quit

use bevy::app::AppExit;
use bevy::core_pipeline::oit::OrderIndependentTransparencySettings;
use bevy::gltf::Gltf;
use bevy::pbr::{ExtendedMaterial, MaterialExtension};
use bevy::prelude::*;
use bevy::reflect::Reflect;
use bevy::render::render_resource::{AsBindGroup, ShaderType};
use bevy::render::renderer::{RenderDevice, RenderQueue};
use bevy::shader::ShaderRef;
use bevy::window::WindowResolution;
// bevy 0.19: glTF scenes are WorldAssets, spawned via WorldAssetRoot.
use bevy::world_serialization::WorldAssetRoot;
use ratatui::prelude::*;
use ratatui::style::Color as RatatuiColor;
use ratatui::widgets::*;
use std::sync::Arc;
use tracing::info;
use unicode_width::UnicodeWidthStr;

use bevy_tui_texture::prelude::*;
use bevy_tui_texture::Font as TerminalFont;

fn main() {
    App::new()
        .add_plugins(DefaultPlugins.set(WindowPlugin {
            primary_window: Some(Window {
                title: "CRT Effect with Mesh3d".to_string(),
                resolution: WindowResolution::new(1024, 768),
                ..default()
            }),
            ..default()
        }))
        .add_plugins(MaterialPlugin::<CrtMaterial>::default())
        .add_plugins(MaterialPlugin::<BlurMaterial>::default())
        .add_plugins(TerminalPlugin::default())
        .add_systems(Startup, setup)
        .add_systems(Update, spawn_gltf_scene_simple)
        .add_systems(Update, (claim_object2_screen, claim_monitor_reflection))
        .add_systems(Update, handle_input.in_set(TerminalSystemSet::UserUpdate))
        .add_systems(
            Update,
            handle_terminal_events.in_set(TerminalSystemSet::UserUpdate),
        )
        .add_systems(
            Update,
            (
                render_terminal.in_set(TerminalSystemSet::Render),
                render_overlay_terminal.in_set(TerminalSystemSet::Render),
                update_camera_rotation,
            ),
        )
        .add_systems(Update, update_crt_uniforms)
        .add_systems(Update, update_blur_uniforms)
        .add_systems(Update, update_directional_light)
        .run();
}

// CRT effect uniforms (matches WGSL memory layout)
#[derive(Clone, Copy, Debug, ShaderType, Reflect)]
struct CrtUniforms {
    effect_intensity: f32,     // 0.0 = off, 1.0 = full effect
    time: f32,                 // For animated scan lines
    scan_line_intensity: f32,  // How pronounced scan lines are
    chromatic_aberration: f32, // RGB channel separation amount
}

// Material extension for CRT effects
#[derive(Asset, AsBindGroup, Clone, Reflect, Debug)]
struct CrtExtension {
    #[uniform(100)] // Binding 100 - safely above StandardMaterial's 0-30 range
    pub uniforms: CrtUniforms,
}

impl MaterialExtension for CrtExtension {
    fn fragment_shader() -> ShaderRef {
        "shaders/crt_extended.wgsl".into()
    }
}

// Convenient type alias for our extended material
type CrtMaterial = ExtendedMaterial<StandardMaterial, CrtExtension>;

// Blur effect uniforms (matches WGSL memory layout)
#[derive(Clone, Copy, Debug, ShaderType, Reflect)]
struct BlurUniforms {
    effect_intensity: f32, // 0.0 = off, 1.0 = full effect
    time: f32,             // For animated effects
    blur_radius: f32,      // Blur radius
    blur_samples: f32,     // Blur sample count
}

// Custom Material implementation for blur effects (shader-only, no PBR)
#[derive(Asset, AsBindGroup, Clone, Debug, TypePath)]
struct BlurMaterial {
    #[uniform(0)]
    pub uniforms: BlurUniforms,

    #[texture(1)]
    #[sampler(2)]
    pub base_color_texture: Option<Handle<Image>>,
    pub alpha_mode: AlphaMode,
}

impl Material for BlurMaterial {
    fn vertex_shader() -> ShaderRef {
        "shaders/unlit_blur.wgsl".into()
    }
    fn fragment_shader() -> ShaderRef {
        "shaders/unlit_blur.wgsl".into()
    }

    fn alpha_mode(&self) -> AlphaMode {
        // self.alpha_mode
        AlphaMode::Add
    }
    fn specialize(
        _pipeline: &bevy::pbr::MaterialPipeline,
        descriptor: &mut bevy::render::render_resource::RenderPipelineDescriptor,
        layout: &bevy::mesh::MeshVertexBufferLayoutRef,
        _key: bevy::pbr::MaterialPipelineKey<Self>,
    ) -> Result<(), bevy::render::render_resource::SpecializedMeshPipelineError> {
        // Explicitly match the vertex layout to the shader's VertexInput.
        // Without this, bevy's default attribute assignment (location 1 =
        // NORMAL) feeds normals into the shader's `color` (location 1),
        // silently breaking vertex colors. COLOR is required here
        // (Monitor_Reflection carries the model's COLOR_0 — a diamond fade,
        // black corners / white edge midpoints — loaded by the glTF loader).
        let vertex_layout = layout.0.get_layout(&[
            Mesh::ATTRIBUTE_POSITION.at_shader_location(0),
            Mesh::ATTRIBUTE_COLOR.at_shader_location(1),
            Mesh::ATTRIBUTE_UV_0.at_shader_location(2),
        ])?;
        descriptor.vertex.buffers = vec![vertex_layout];

        // Disable depth writes (bevy 0.19: Option<bool>)
        if let Some(depth_stencil) = &mut descriptor.depth_stencil {
            depth_stencil.depth_write_enabled = Some(false);
        }

        // Explicit additive blend state
        if let Some(fragment) = &mut descriptor.fragment {
            for target in fragment.targets.iter_mut() {
                if let Some(color_target) = target {
                    // Additive blending
                    color_target.blend = Some(bevy::render::render_resource::BlendState {
                        color: bevy::render::render_resource::BlendComponent {
                            src_factor: bevy::render::render_resource::BlendFactor::One,
                            dst_factor: bevy::render::render_resource::BlendFactor::One,
                            operation: bevy::render::render_resource::BlendOperation::Add,
                        },
                        alpha: bevy::render::render_resource::BlendComponent {
                            src_factor: bevy::render::render_resource::BlendFactor::One,
                            dst_factor: bevy::render::render_resource::BlendFactor::One,
                            operation: bevy::render::render_resource::BlendOperation::Add,
                        },
                    });
                }
            }
        }

        Ok(())
    }
}

// ReflectionMaterial removed - additive blending implemented via StandardMaterial

#[derive(Clone, Copy, Debug, PartialEq)]
enum CameraMode {
    MouseFollow, // Follows the mouse (+-30 degrees)
    Fixed,       // Fixed front view
    Orbit,       // Orbits the model (current default)
}

// Helper functions for UI elements
fn checkbox_span(checked: bool) -> Span<'static> {
    Span::styled(
        if checked { "[X]" } else { "[ ]" },
        if checked {
            Style::default().fg(RatatuiColor::Green).bold()
        } else {
            Style::default().fg(RatatuiColor::Gray)
        },
    )
}

fn radio_span(selected: bool) -> Span<'static> {
    Span::styled(
        if selected { "(o)" } else { "( )" },
        if selected {
            Style::default().fg(RatatuiColor::Green).bold()
        } else {
            Style::default().fg(RatatuiColor::Gray)
        },
    )
}

/// Marker for the overlay `Tui` entity.
#[derive(Component)]
struct OverlayScreen;

// CRT screen tabs bar: labels, in display order.
const TAB_TITLES: [&str; 3] = ["STATUS", "EFFECTS", "TABLE"];

/// Click regions across both terminals. Each `Tui` has its own
/// `HitRegions` registry, so there's no collision risk in sharing one enum
/// between the overlay and the CRT screen.
#[derive(Clone, Copy, Debug)]
enum Hit {
    Tab(u8),
    Button,
    TableRow(u8),
    CrtCheckbox,
    ShadowsCheckbox,
    CameraRadio(u8),
}

impl From<Hit> for u64 {
    fn from(h: Hit) -> u64 {
        match h {
            Hit::Tab(i) => 0x01_00 | i as u64,
            Hit::Button => 0x02_00,
            Hit::TableRow(i) => 0x03_00 | i as u64,
            Hit::CrtCheckbox => 0x04_00,
            Hit::ShadowsCheckbox => 0x05_00,
            Hit::CameraRadio(i) => 0x06_00 | i as u64,
        }
    }
}

impl TryFrom<u64> for Hit {
    type Error = ();
    fn try_from(v: u64) -> Result<Self, ()> {
        match v & 0xff_00 {
            0x01_00 => Ok(Hit::Tab((v & 0xff) as u8)),
            0x02_00 => Ok(Hit::Button),
            0x03_00 => Ok(Hit::TableRow((v & 0xff) as u8)),
            0x04_00 => Ok(Hit::CrtCheckbox),
            0x05_00 => Ok(Hit::ShadowsCheckbox),
            0x06_00 => Ok(Hit::CameraRadio((v & 0xff) as u8)),
            _ => Err(()),
        }
    }
}

#[derive(Resource)]
struct AppState {
    effects_enabled: bool,
    frame_count: u32,
    fps: f32,
    button_clicked: bool,
    button_click_count: u32,
    light_illuminance: f32,
    shadows_enabled: bool,
    camera_mode: CameraMode,
    // CRT screen tabs (STATUS / EFFECTS / TABLE), switchable by click or arrow keys
    selected_tab: usize,
    // TABLE tab: mouse-selectable diagnostics table
    table_state: TableState,
}

impl Default for AppState {
    fn default() -> Self {
        Self {
            effects_enabled: true,
            frame_count: 0,
            fps: 0.0,
            button_clicked: false,
            button_click_count: 0,
            // Dim ambient light so the CRT screen (unlit + emissive) stands out
            light_illuminance: 1500.0,
            shadows_enabled: true, // shadows on by default
            camera_mode: CameraMode::Orbit, // default: orbiting camera
            selected_tab: 0,
            table_state: TableState::default(),
        }
    }
}

/// Marker component for ground mesh
#[derive(Component)]
struct GroundMesh;

/// Marker component for the camera that orbits around the computer
#[derive(Component)]
struct OrbitCamera;

/// Marker component for the main directional light
#[derive(Component)]
struct MainDirectionalLight;

/// Marker component for model
#[derive(Component)]
struct GltfModel;

/// GLTF asset handle component
#[derive(Component)]
struct GltfAsset(Handle<Gltf>);

/// Marker for the main CRT screen's `Tui` entity. Object_2's mesh attaches
/// to this via `AttachTerminal`, rather than carrying the `Tui` itself. Its
/// material handle is found via `Query<&MeshMaterial3d<CrtMaterial>>` on
/// Object_2 directly - no separate marker component needed.
#[derive(Component)]
struct MainScreen;

/// Marker component to indicate GLTF has been spawned
#[derive(Component)]
struct GltfSpawned;

fn setup(
    mut commands: Commands,
    render_device: Res<RenderDevice>,
    render_queue: Res<RenderQueue>,
    mut images: ResMut<Assets<Image>>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut standard_materials: ResMut<Assets<StandardMaterial>>,
    asset_server: Res<AssetServer>,
) {
    // Load font
    let font_data = include_bytes!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/assets/fonts/fusion-pixel-10px-monospaced-ja.ttf"
    ));
    let font = TerminalFont::new(font_data).expect("Failed to load font");
    let fonts = Arc::new(Fonts::new(font, 16));

    // Create main terminal texture
    let mut texture = TerminalTexture::create(
        32,
        24,
        fonts.clone(),
        true,
        &render_device,
        &render_queue,
        &mut images,
    )
    .expect("Failed to create main terminal");

    // Camera with Order Independent Transparency (MSAA disabled for OIT compatibility)
    commands.spawn((
        Camera3d::default(),
        Transform::from_xyz(0.0, 0.5, 1.2).looking_at(Vec3::new(0.0, 0.1, 0.0), Vec3::Y),
        OrderIndependentTransparencySettings::default(), // OIT enabled
        Msaa::Off,                                       // OIT is incompatible with MSAA
        OrbitCamera,
    ));

    // Note: Materials are now created dynamically when needed
    // (CrtMaterial for Object_2 and ReflectionMaterial for Monitor_Reflection)

    // / Ground Mesh Starts
    // Create ground material (StandardMaterial that uses vertex colors)
    let ground_material = StandardMaterial {
        base_color: bevy::color::Color::WHITE, // White base color to show vertex colors accurately
        metallic: 0.0,
        perceptual_roughness: 0.8, // Rough surface
        reflectance: 0.1,          // Low reflectance
        // Note: StandardMaterial automatically multiplies base_color by vertex color
        ..default()
    };
    let ground_material_handle = standard_materials.add(ground_material);

    // Create circular ground mesh with radial vertex colors (Y-up circle)
    let mut ground_mesh = Mesh::from(Circle::new(3.0)); // 3.0 radius circle

    // Add radial vertex colors (light center -> dark edge)
    if let Some(positions) = ground_mesh
        .attribute(Mesh::ATTRIBUTE_POSITION)
        .and_then(|p| p.as_float3())
    {
        let mut colors = Vec::new();

        for position in positions {
            // Calculate distance from center (X-Z plane)
            let distance = (position[0] * position[0] + position[2] * position[2]).sqrt();
            let normalized_distance = (distance / 3.0).clamp(0.0, 1.0);

            // Dim environment: gradient from center (0.22) to edge (0.02)
            // (a bright floor would compete with the CRT screen)
            let brightness = 0.22 - (normalized_distance * 0.20); // 0.22 -> 0.02
            colors.push([brightness, brightness, brightness, 1.0]);
        }

        ground_mesh.insert_attribute(Mesh::ATTRIBUTE_COLOR, colors);
    }

    let ground_mesh_handle = meshes.add(ground_mesh);

    // Spawn circular ground mesh with StandardMaterial
    commands.spawn((
        Mesh3d(ground_mesh_handle),
        MeshMaterial3d(ground_material_handle),
        Transform::from_rotation(Quat::from_rotation_x(-std::f32::consts::FRAC_PI_2)),
        GroundMesh,
    ));
    // / Ground Mesh Over

    // Directional light (dim, shadows on; update_directional_light syncs
    // from AppState every frame, so match its initial values here too)
    commands.spawn((
        DirectionalLight {
            illuminance: 1500.0,
            shadow_maps_enabled: true, // bevy 0.19 rename
            ..default()
        },
        Transform::from_rotation(Quat::from_euler(EulerRot::XYZ, -0.5, -0.5, 0.0)),
        MainDirectionalLight,
    ));

    // Dim the surroundings so the CRT screen (unlit + emissive) stands out
    commands.insert_resource(ClearColor(bevy::color::Color::srgb(0.015, 0.015, 0.025)));

    // Load the whole glTF (as an asset, not scene-based)
    let gltf_handle: Handle<Gltf> = asset_server.load("models/retro_crt.glb");
    commands.spawn((GltfAsset(gltf_handle), GltfModel));

    // Initial synchronous render for main terminal (prevents first-frame
    // black texture) via `TerminalTexture::draw_sync`.
    texture.draw_sync(&render_device, &render_queue, &mut images, |frame| {
        let area = frame.area();

        // Clear with colorful background
        let clear = Block::default().style(Style::default().bg(RatatuiColor::Rgb(10, 10, 30)));
        frame.render_widget(clear, area);

        let title = Paragraph::new("Shader with ExtendedMaterial - Loading...")
            .style(
                Style::default()
                    .fg(RatatuiColor::Green)
                    .bg(RatatuiColor::DarkGray)
                    .bold(),
            )
            .alignment(Alignment::Center)
            .block(Block::bordered().border_style(Style::default().fg(RatatuiColor::White)));
        frame.render_widget(title, area);
    });

    // Overlay terminal: a `Tui` entity, top-right anchored via `Val::Px`
    // alone — no px-estimate math, no resize system needed; bevy_ui
    // re-anchors `right`/`top` automatically.
    let mut ctx = TerminalSpawnCtx {
        render_device: &render_device,
        render_queue: &render_queue,
        images: &mut images,
        meshes: &mut meshes,
        materials: &mut standard_materials,
    };
    let overlay_bundle = TerminalBundle::ui(
        32, // columns (slightly smaller)
        21, // rows (control UI + model credit lines)
        fonts.clone(),
        TerminalConfig::default(),
        &mut ctx,
    )
    .expect("Failed to create overlay terminal");

    commands.spawn((
        overlay_bundle,
        Node {
            position_type: PositionType::Absolute,
            right: Val::Px(20.0),
            top: Val::Px(20.0),
            ..default()
        },
        OverlayScreen,
    ));

    // The main CRT screen's terminal is a `Tui` entity, not a Resource.
    // Object_2's mesh attaches to it via `AttachTerminal` once the glTF
    // loads (claim_object2_screen below), found by querying the
    // `MainScreen` marker rather than a resource.
    commands.spawn((Tui::from_texture_state(texture), MainScreen));
    commands.insert_resource(AppState::default());
}

// Simple, efficient glTF scene spawn (only checks assets not yet spawned)
fn spawn_gltf_scene_simple(
    mut commands: Commands,
    gltf_query: Query<(Entity, &GltfAsset), (With<GltfModel>, Without<GltfSpawned>)>,
    gltf_assets: Res<Assets<Gltf>>,
) {
    // Nothing to do if there are no unspawned glTF assets
    if gltf_query.is_empty() {
        return;
    }

    for (gltf_entity, gltf_asset) in gltf_query.iter() {
        // Check whether the glTF asset has finished loading (once)
        let Some(gltf) = gltf_assets.get(&gltf_asset.0) else {
            continue; // not loaded yet
        };

        info!(
            "GLTF asset loaded! Found {} scenes, spawning...",
            gltf.scenes.len()
        );

        // Spawn the default (first) scene, rotated 90° clockwise
        // bevy 0.19: Gltf::scenes is Vec<Handle<WorldAsset>> → WorldAssetRoot.
        if let Some(scene) = gltf.scenes.first() {
            commands.spawn((
                WorldAssetRoot(scene.clone()),
                Transform::from_rotation(Quat::from_rotation_y(-std::f32::consts::FRAC_PI_2)),
            ));
            info!("GLTF scene spawned successfully with 90-degree clockwise rotation");
        }

        // Mark as spawned to prevent re-running
        commands.entity(gltf_entity).insert(GltfSpawned);
    }
}

/// Claims Object_2 (Monitor Glass) by inserting `AttachTerminal`, pointing
/// it at the main screen's `Tui`. The library's own `attach_terminal_system`
/// then does the actual material swap and handles the glTF loader
/// asynchronously re-inserting its stock `StandardMaterial` over ours on a
/// later frame (see CLAUDE.md "Common
/// Gotchas" #8) - this system just needs to (re-)insert `AttachTerminal`
/// for as long as the entity still carries `StandardMaterial`, which is
/// harmless to repeat (same component value each time).
fn claim_object2_screen(
    to_claim: Query<(Entity, &Name, &MeshMaterial3d<StandardMaterial>)>,
    standard_materials: Res<Assets<StandardMaterial>>,
    main_screen: Query<Entity, With<MainScreen>>,
    mut commands: Commands,
) {
    if std::env::var("CRT_LIST_NODES").is_ok() {
        for (_, name, _) in &to_claim {
            info!("mesh entity: '{}'", name.as_str());
        }
    }

    let Ok(main_screen) = main_screen.single() else {
        return; // main screen Tui not spawned yet - try again next frame
    };

    for (entity, name, standard_material_handle) in &to_claim {
        // bevy's glTF loader names primitive entities "<mesh name>.<material
        // name>" (e.g. "Object_2.Monitor_Glass"). The glass is mesh
        // "Object_2" × material "Monitor_Glass". (A bare prefix match on
        // "Object_2" would also hit "Object_20" etc. in other models, so we
        // match through the ".".)
        if name.as_str() != "Object_2.Monitor_Glass" {
            continue;
        }
        let Some(standard_material) = standard_materials.get(standard_material_handle) else {
            continue;
        };

        info!("Found Object_2 (Monitor Glass): '{}' - attaching Tui", name.as_str());

        // Captured for the material factory below - the glTF material's own
        // double_sided/cull_mode should carry over even though everything
        // else is overridden (see the factory's doc comment).
        let double_sided = standard_material.double_sided;
        let cull_mode = standard_material.cull_mode;

        // Note: this model's (_0) glass UVs span the full [0,1]² range and
        // are already upright, so no correction is needed (verified with
        // the CRT_CALIBRATE=1 quadrant display). If a different model's
        // orientation is off, rewrite the mesh UVs directly instead of
        // using uv_transform, which would desync display from mouse
        // picking.
        commands.entity(entity).insert(AttachTerminal {
            terminal: main_screen,
            material: AttachMaterial::custom(move |image| CrtMaterial {
                base: StandardMaterial {
                    base_color: bevy::color::Color::WHITE,
                    base_color_texture: Some(image),
                    unlit: false,
                    alpha_mode: AlphaMode::Opaque,
                    double_sided,
                    cull_mode,
                    // Dielectric glass, not the glTF material's own values:
                    // low roughness + raised reflectance so the
                    // DirectionalLight's PBR specular highlight ("light
                    // reflected in the screen") reads clearly through
                    // apply_pbr_lighting, on top of which the scan-line/
                    // vignette pass in crt_extended.wgsl still multiplies
                    // unchanged.
                    metallic: 0.0,
                    perceptual_roughness: 0.15,
                    reflectance: 0.9,
                    ..default()
                },
                extension: CrtExtension {
                    uniforms: CrtUniforms {
                        effect_intensity: 1.0,
                        time: 0.0,
                        scan_line_intensity: 0.1,
                        chromatic_aberration: 0.002,
                    },
                },
            }),
        });
    }
}

/// Monitor_Reflection is a decorative overlay reusing the terminal's
/// texture for an unlit additive glow - it is NOT an interactive terminal
/// surface, so it does not go through `AttachTerminal` (which would
/// incorrectly grant it `TuiSurface`/`TerminalInput` and make it compete
/// for click hit-testing with the real screen). It still needs its own
/// persistent re-claim loop for the same async-glTF-loader reason
/// `AttachTerminal` has one - see `claim_object2_screen`'s doc comment.
fn claim_monitor_reflection(
    to_claim: Query<(Entity, &Name, &MeshMaterial3d<StandardMaterial>)>,
    mut blur_materials: ResMut<Assets<BlurMaterial>>,
    main_screen: Query<&Tui, With<MainScreen>>,
    mut commands: Commands,
) {
    let Ok(main_tui) = main_screen.single() else {
        return;
    };

    for (entity, name, _) in &to_claim {
        let name_str = name.as_str();
        if !name_str.contains("Monitor_Reflection") && !name_str.contains("Reflection") {
            continue;
        }

        info!("Found Monitor_Reflection: '{}' - attaching BlurMaterial", name_str);

        let blur_material_handle = blur_materials.add(BlurMaterial {
            uniforms: BlurUniforms {
                effect_intensity: 1.0, // ON (update system toggles 0/1)
                time: 0.0,             // Will be updated in update system
                blur_radius: 3.0,      // Medium blur radius
                blur_samples: 5.0,     // 5x5 kernel
            },
            base_color_texture: Some(main_tui.image_handle().clone()),
            alpha_mode: AlphaMode::Add, // additive blending
        });

        // Vertex colors already come from the model (the GLB's COLOR_0):
        // black at the corners/edges, white only at each edge's midpoint -
        // a diamond fade. The glTF loader imports this as ATTRIBUTE_COLOR,
        // so generating/overwriting it here is unnecessary.
        commands
            .entity(entity)
            .remove::<MeshMaterial3d<StandardMaterial>>()
            .insert(MeshMaterial3d(blur_material_handle))
            // The additive reflection surface doesn't participate in
            // shadows. Without this, enabling shadows makes the prepass
            // pipeline choke on the custom vertex layout (POSITION/COLOR/
            // UV) with a Validation Error.
            .insert((bevy::light::NotShadowCaster, bevy::light::NotShadowReceiver));
    }
}

fn handle_input(
    keys: Res<ButtonInput<KeyCode>>,
    mut app_state: ResMut<AppState>,
    mut app_exit: MessageWriter<AppExit>,
) {
    if keys.just_pressed(KeyCode::Escape) {
        app_exit.write(AppExit::Success);
    }
    if keys.just_pressed(KeyCode::Space) {
        app_state.effects_enabled = !app_state.effects_enabled;
        info!(
            "CRT effects: {}",
            if app_state.effects_enabled {
                "ON"
            } else {
                "OFF"
            }
        );
    }
    // Switch the CRT screen's tabs (STATUS / EFFECTS / TABLE)
    if keys.just_pressed(KeyCode::ArrowRight) {
        app_state.selected_tab = (app_state.selected_tab + 1) % 3;
    }
    if keys.just_pressed(KeyCode::ArrowLeft) {
        app_state.selected_tab = (app_state.selected_tab + 2) % 3;
    }
}

fn handle_terminal_events(
    mut events: MessageReader<TerminalEvent>,
    mut app_state: ResMut<AppState>,
    main_screen_query: Query<(Entity, &Tui), With<MainScreen>>,
    overlay_query: Query<(Entity, &Tui), With<OverlayScreen>>,
) {
    let Ok((main_entity, main_tui)) = main_screen_query.single() else {
        return; // main screen Tui not created yet
    };
    let overlay = overlay_query.single().ok();

    for event in events.read() {
        if event.target == main_entity {
            if let TerminalEventType::MousePress { position, .. } = &event.event {
                info!(
                    "Object_2 mouse click at col={}, row={}",
                    position.0, position.1
                );

                // Hit-tested via HitRegions, not parallel Rect bookkeeping.
                // Button/TableRow hits can only
                // occur when the STATUS/TABLE tab is active anyway, since
                // HitRegions is rebuilt fresh every draw and only the
                // active tab's render function runs.
                match main_tui.hit_regions().hit_at::<Hit>(*position) {
                    Some(Hit::Tab(tab)) => {
                        app_state.selected_tab = tab as usize;
                        info!("🎯 Tab switched: {}", tab);
                    }
                    Some(Hit::Button) => {
                        app_state.button_clicked = true;
                        app_state.button_click_count += 1;
                        info!("🎯 Button clicked! Count: {}", app_state.button_click_count);
                    }
                    Some(Hit::TableRow(row)) => {
                        app_state.table_state.select(Some(row as usize));
                        info!("🎯 Table row selected: {}", row);
                    }
                    _ => {}
                }
            }
        } else if let Some((_, overlay_tui)) =
            overlay.filter(|(entity, _)| *entity == event.target)
        {
            // Overlay terminal events - hit-tested via HitRegions, not
            // parallel Rect bookkeeping.
            if let TerminalEventType::MousePress { position, .. } = &event.event {
                info!(
                    "Overlay terminal mouse click at col={}, row={}",
                    position.0, position.1
                );

                match overlay_tui.hit_regions().hit_at::<Hit>(*position) {
                    Some(Hit::CrtCheckbox) => {
                        app_state.effects_enabled = !app_state.effects_enabled;
                        info!(
                            "🎯 CRT Effects: {}",
                            if app_state.effects_enabled {
                                "ON"
                            } else {
                                "OFF"
                            }
                        );
                    }
                    Some(Hit::ShadowsCheckbox) => {
                        app_state.shadows_enabled = !app_state.shadows_enabled;
                        info!(
                            "🎯 Shadows: {}",
                            if app_state.shadows_enabled {
                                "ON"
                            } else {
                                "OFF"
                            }
                        );
                    }
                    Some(Hit::CameraRadio(i)) => {
                        let new_mode = [
                            CameraMode::MouseFollow,
                            CameraMode::Fixed,
                            CameraMode::Orbit,
                        ][i as usize];
                        if app_state.camera_mode != new_mode {
                            app_state.camera_mode = new_mode;
                            info!("🎯 Camera: {:?}", new_mode);
                        }
                    }
                    _ => {}
                }
            }
        } else {
            info!(
                "Event target {:?} does not match any known terminal entity",
                event.target
            );
        }
    }
}

fn update_crt_uniforms(
    mut crt_materials: ResMut<Assets<CrtMaterial>>,
    object2_query: Query<&MeshMaterial3d<CrtMaterial>>,
    app_state: Res<AppState>,
    time: Res<Time>,
) {
    // Update Object_2 (Monitor Glass) CRT material if it exists.
    // (bevy 0.19: get_mut returns an AssetMut guard, hence `mut`)
    for material_handle in &object2_query {
        if let Some(mut material) = crt_materials.get_mut(&material_handle.0) {
            material.extension.uniforms.effect_intensity =
                if app_state.effects_enabled { 1.0 } else { 0.0 };
            material.extension.uniforms.time = time.elapsed_secs();
        }
    }

    // Monitor_Reflection already uses BlurMaterial - its uniforms are updated below
}

fn update_blur_uniforms(
    mut blur_materials: ResMut<Assets<BlurMaterial>>,
    reflection_query: Query<&MeshMaterial3d<BlurMaterial>>,
    app_state: Res<AppState>,
    time: Res<Time>,
) {
    // Update reflection blur material if it exists.
    // (bevy 0.19: get_mut returns an AssetMut guard, hence `mut`)
    for material_handle in &reflection_query {
        if let Some(mut material) = blur_materials.get_mut(&material_handle.0) {
            material.uniforms.effect_intensity = if app_state.effects_enabled { 1.0 } else { 0.0 };
            material.uniforms.time = time.elapsed_secs();
        }
    }
}

/// Zero render-resource parameters: the plugin's `gpu_flush_system` owns
/// the GPU render + async copy + material touch.
fn render_terminal(
    mut screens: Query<&mut Tui, With<MainScreen>>,
    mut app_state: ResMut<AppState>,
    time: Res<Time>,
) {
    let Ok(mut term) = screens.single_mut() else {
        return;
    };
    app_state.frame_count += 1;

    // Exponential moving average smooths frame-to-frame jitter so the
    // overlay's FPS readout doesn't flicker every frame.
    let dt = time.delta_secs();
    if dt > 0.0 {
        let instant_fps = 1.0 / dt;
        app_state.fps = if app_state.fps <= 0.0 {
            instant_fps
        } else {
            app_state.fps * 0.9 + instant_fps * 0.1
        };
    }

    // Button click animation (resets after 1 second)
    if app_state.button_clicked && app_state.frame_count % 60 == 0 {
        app_state.button_clicked = false;
    }

    // UV calibration: CRT_CALIBRATE=1 shows four quadrants + a counter
    // (used to measure the screen's UV orientation and visible region)
    if std::env::var("CRT_CALIBRATE").is_ok() {
        let count = app_state.frame_count / 10;
        term.draw(|frame| {
                let a = frame.area();
                let (hw, hh) = (a.width / 2, a.height / 2);
                let quads = [
                    (0, 0, hw, hh, RatatuiColor::Red, "R-TL"),
                    (hw, 0, a.width - hw, hh, RatatuiColor::Green, "G-TR"),
                    (0, hh, hw, a.height - hh, RatatuiColor::Blue, "B-BL"),
                    (
                        hw,
                        hh,
                        a.width - hw,
                        a.height - hh,
                        RatatuiColor::Yellow,
                        "Y-BR",
                    ),
                ];
                for (x, y, w, h, color, label) in quads {
                    frame.render_widget(
                        Paragraph::new(label)
                            .style(Style::default().fg(RatatuiColor::Black).bg(color)),
                        ratatui::layout::Rect {
                            x,
                            y,
                            width: w,
                            height: h,
                        },
                    );
                }
                frame.render_widget(
                    Paragraph::new(format!("{count:^10}")).style(
                        Style::default()
                            .fg(RatatuiColor::White)
                            .bg(RatatuiColor::Black),
                    ),
                    ratatui::layout::Rect {
                        x: a.width / 2 - 5,
                        y: hh,
                        width: 10,
                        height: 1,
                    },
                );
            });
        return;
    }

    // Everything below is laid out to fit EXACTLY inside the 32x24 terminal
    // grid (see TerminalTexture::create in setup()): a 1-cell outer border
    // leaves a 30x22 interior, split into a Ratatui logo banner, a rule,
    // a switchable Tabs bar, 17 rows of tab content, and a marquee footer.
    term.draw_with_hits(|frame, hits| {
            let area = frame.area();

            let outer = Block::bordered()
                .border_style(Style::default().fg(RatatuiColor::Cyan))
                .style(Style::default().bg(RatatuiColor::Black))
                .title(Line::styled(
                    " TERMINAL-32 ",
                    Style::default().fg(RatatuiColor::Magenta).bold(),
                ))
                .title_bottom(Line::styled(
                    "SPACE:FX CLICK:SELECT ESC:QUIT",
                    Style::default().fg(RatatuiColor::Gray),
                ));
            let interior = outer.inner(area);
            frame.render_widget(outer, area);

            let sections = Layout::vertical([
                Constraint::Length(2),  // banner: RatatuiLogo::small()
                Constraint::Length(1),  // separator rule
                Constraint::Length(1),  // blank (above tabs bar)
                Constraint::Length(1),  // tabs bar
                Constraint::Length(1),  // blank (below tabs bar)
                Constraint::Length(15), // tab content
                Constraint::Length(1),  // marquee footer
            ])
            .split(interior);
            let (banner_area, rule_area, tabs_area, content_area, marquee_area) = (
                sections[0],
                sections[1],
                sections[3],
                sections[5],
                sections[6],
            );

            // --- Banner: the official Ratatui logo, in retro rainbow bands ---
            let banner_cols = Layout::horizontal([
                Constraint::Fill(1),
                Constraint::Length(27),
                Constraint::Fill(1),
            ])
            .split(banner_area);
            let logo_rect = banner_cols[1];
            let band_colors = [
                RatatuiColor::Magenta,
                RatatuiColor::Cyan,
                RatatuiColor::Yellow,
            ];
            let band_w = logo_rect.width / band_colors.len() as u16;
            for (i, color) in band_colors.iter().enumerate() {
                let w = if i == band_colors.len() - 1 {
                    logo_rect.width - band_w * (band_colors.len() as u16 - 1)
                } else {
                    band_w
                };
                let band = ratatui::layout::Rect {
                    x: logo_rect.x + band_w * i as u16,
                    y: logo_rect.y,
                    width: w,
                    height: logo_rect.height,
                };
                // A plain color fill; RatatuiLogo's glyphs are drawn with
                // Style::default() (no fg/bg set), so they inherit whatever
                // color is already in each cell - giving a banded rainbow.
                frame.render_widget(
                    Block::default().style(Style::default().bg(RatatuiColor::Black).fg(*color)),
                    band,
                );
            }
            frame.render_widget(RatatuiLogo::small(), logo_rect);

            // --- Separator rule ---
            frame.render_widget(
                Paragraph::new("═".repeat(rule_area.width as usize))
                    .style(Style::default().fg(RatatuiColor::DarkGray)),
                rule_area,
            );

            // --- Tabs bar: https://ratatui.rs/examples/widgets/tabs/ ---
            // Switchable by mouse click (hit-tested below) or Left/Right arrow keys.
            const TAB_DIVIDER: &str = "|";
            const TAB_PAD: &str = " ";
            let tabs = Tabs::new(TAB_TITLES)
                .style(
                    Style::default()
                        .fg(RatatuiColor::Gray)
                        .bg(RatatuiColor::Black),
                )
                .highlight_style(
                    Style::default()
                        .fg(RatatuiColor::Black)
                        .bg(RatatuiColor::Magenta)
                        .bold(),
                )
                .select(app_state.selected_tab)
                .divider(TAB_DIVIDER)
                .padding(TAB_PAD, TAB_PAD);
            frame.render_widget(tabs, tabs_area);

            // Click zones sized to each title's actual rendered width (padding
            // + label text), matching how Tabs itself lays the header out
            // left-to-right, so hit-testing tracks the real glyph boundaries
            // rather than an equal three-way split.
            let pad_w = TAB_PAD.width() as u16;
            let divider_w = TAB_DIVIDER.width() as u16;
            let mut x = tabs_area.x;
            for (i, title) in TAB_TITLES.iter().enumerate() {
                let seg_w = pad_w + title.width() as u16 + pad_w;
                let clamped_w = seg_w.min(tabs_area.right().saturating_sub(x));
                hits.add(
                    Hit::Tab(i as u8),
                    ratatui::layout::Rect {
                        x,
                        y: tabs_area.y,
                        width: clamped_w,
                        height: tabs_area.height,
                    },
                );
                x += seg_w + divider_w;
            }

            // --- Tab content ---
            match app_state.selected_tab {
                0 => render_status_tab(frame, content_area, &app_state, hits),
                1 => render_effects_tab(frame, content_area),
                _ => render_table_tab(frame, content_area, &mut app_state, hits),
            }

            // --- Marquee footer: classic BBS "chasing lights" scroller (edge animation) ---
            frame.render_widget(
                marquee_line(app_state.frame_count, marquee_area.width),
                marquee_area,
            );
        });
}

/// STATUS tab: the interactive button (click to increment) + a live status readout.
fn render_status_tab(
    frame: &mut ratatui::Frame,
    area: ratatui::layout::Rect,
    app_state: &AppState,
    hits: &mut HitRegions,
) {
    let rows = Layout::vertical([
        Constraint::Length(3), // button
        Constraint::Length(1), // blank
        Constraint::Length(1), // FX
        Constraint::Length(1), // SHADOW
        Constraint::Length(1), // LIGHT
        Constraint::Length(1), // CAM
        Constraint::Length(1), // FRAME
        Constraint::Length(1), // CLICKS
        Constraint::Min(0),    // filler
    ])
    .split(area);

    let button_style = if app_state.button_clicked {
        Style::default()
            .bg(RatatuiColor::Yellow)
            .fg(RatatuiColor::Black)
            .bold()
    } else {
        Style::default()
            .bg(RatatuiColor::DarkGray)
            .fg(RatatuiColor::White)
    };
    let button = Paragraph::new(format!("Click Me! (x{})", app_state.button_click_count))
        .style(button_style)
        .alignment(Alignment::Center)
        .block(
            Block::bordered()
                .border_style(Style::default().fg(RatatuiColor::Magenta))
                .title("BTN"),
        );
    frame.render_widget(button, rows[0]);

    // Register the hit-test area (inside the border)
    hits.add(
        Hit::Button,
        ratatui::layout::Rect {
            x: rows[0].x + 1,
            y: rows[0].y + 1,
            width: rows[0].width.saturating_sub(2),
            height: rows[0].height.saturating_sub(2),
        },
    );

    let cam = match app_state.camera_mode {
        CameraMode::MouseFollow => "MOUSE",
        CameraMode::Fixed => "FIXED",
        CameraMode::Orbit => "ORBIT",
    };
    let lines = [
        format!(
            "FX     : {}",
            if app_state.effects_enabled {
                "ON"
            } else {
                "OFF"
            }
        ),
        format!(
            "SHADOW : {}",
            if app_state.shadows_enabled {
                "ON"
            } else {
                "OFF"
            }
        ),
        format!("LIGHT  : {:.0}k", app_state.light_illuminance / 1000.0),
        format!("CAM    : {cam}"),
        format!("FRAME  : {}", app_state.frame_count),
        format!("CLICKS : {}", app_state.button_click_count),
    ];
    for (i, line) in lines.iter().enumerate() {
        frame.render_widget(
            Paragraph::new(line.as_str()).style(Style::default().fg(RatatuiColor::Green)),
            rows[2 + i],
        );
    }
}

/// EFFECTS tab: the CRT effects list + a color test strip.
fn render_effects_tab(frame: &mut ratatui::Frame, area: ratatui::layout::Rect) {
    let lines = vec![
        Line::from(Span::styled(
            "CRT EFFECTS",
            Style::default().fg(RatatuiColor::Yellow).bold(),
        )),
        Line::from(""),
        Line::from(vec![
            Span::raw("* Scanlines  - "),
            Span::styled("flicker", Style::default().fg(RatatuiColor::LightRed)),
        ]),
        Line::from(vec![
            Span::raw("* Vignette   - "),
            Span::styled("dark edges", Style::default().fg(RatatuiColor::LightGreen)),
        ]),
        Line::from(vec![
            Span::raw("* Phosphor   - "),
            Span::styled("glow", Style::default().fg(RatatuiColor::LightBlue)),
        ]),
        Line::from(vec![
            Span::raw("* Colorshift - "),
            Span::styled("RGB split", Style::default().fg(RatatuiColor::LightMagenta)),
        ]),
        Line::from(""),
        Line::from(Span::styled(
            "COLOR TEST",
            Style::default().fg(RatatuiColor::Cyan).bold(),
        )),
        Line::from(vec![
            Span::styled("█████ ", Style::default().fg(RatatuiColor::Red)),
            Span::styled("█████ ", Style::default().fg(RatatuiColor::Green)),
            Span::styled("█████ ", Style::default().fg(RatatuiColor::Blue)),
            Span::styled("█████", Style::default().fg(RatatuiColor::Yellow)),
        ]),
    ];
    frame.render_widget(Paragraph::new(lines), area);
}

/// TABLE tab: https://ratatui.rs/examples/widgets/table/ - a mock diagnostics
/// table whose rows can be selected with a mouse click.
fn render_table_tab(
    frame: &mut ratatui::Frame,
    area: ratatui::layout::Rect,
    app_state: &mut AppState,
    hits: &mut HitRegions,
) {
    let rows_layout = Layout::vertical([
        Constraint::Length(1), // caption
        Constraint::Length(1), // hint
        Constraint::Length(1), // blank
        Constraint::Length(8), // table: header(1) + margin(1) + 5 rows + footer(1)
        Constraint::Min(0),    // filler
    ])
    .split(area);

    frame.render_widget(
        Paragraph::new(Span::styled(
            "SYSTEM DIAGNOSTICS",
            Style::default().fg(RatatuiColor::Cyan).bold(),
        )),
        rows_layout[0],
    );
    frame.render_widget(
        Paragraph::new(Span::styled(
            "click a row to select",
            Style::default().fg(RatatuiColor::DarkGray),
        )),
        rows_layout[1],
    );

    let table_area = rows_layout[3];
    let header = Row::new(["PART", "STATE", "READ"])
        .style(
            Style::default()
                .fg(RatatuiColor::Black)
                .bg(RatatuiColor::Cyan)
                .bold(),
        )
        .bottom_margin(1);
    let data_rows = [
        Row::new(["CRT TUBE", "OK", "87%"]),
        Row::new(["PHOSPHOR", "WARM", "62C"]),
        Row::new(["V-SYNC", "LOCK", "60Hz"]),
        Row::new(["H-SYNC", "LOCK", "31kHz"]),
        Row::new(["DEGAUSS", "READY", "-"]),
    ];
    let footer = Row::new(["SYS STATUS", "", "NOMINAL"])
        .style(Style::default().fg(RatatuiColor::Green).bold());
    let widths = [
        Constraint::Length(12),
        Constraint::Length(8),
        Constraint::Length(8),
    ];
    let row_count = data_rows.len() as u16;
    let table = Table::new(data_rows, widths)
        .header(header)
        .footer(footer)
        .column_spacing(1)
        .style(
            Style::default()
                .fg(RatatuiColor::White)
                .bg(RatatuiColor::Black),
        )
        .row_highlight_style(
            Style::default()
                .fg(RatatuiColor::Yellow)
                .bg(RatatuiColor::Rgb(60, 0, 60))
                .bold(),
        )
        .highlight_spacing(HighlightSpacing::Never);
    frame.render_stateful_widget(table, table_area, &mut app_state.table_state);

    // Register per-row hit regions, mirroring Table's own internal layout:
    // header height 1 + header bottom_margin 1, then each data row is
    // exactly 1 cell tall with no inter-row margin.
    for i in 0..row_count {
        hits.add(
            Hit::TableRow(i as u8),
            ratatui::layout::Rect {
                x: table_area.x,
                y: table_area.y + 2 + i,
                width: table_area.width,
                height: 1,
            },
        );
    }
}

/// A classic BBS-style scrolling marquee with "chasing lights" - an
/// animation that lives right at the bottom edge of the screen.
fn marquee_line(frame_count: u32, width: u16) -> Line<'static> {
    const MSG: &str =
        "*** TERMINAL-32 DEMO *** GPU-ACCELERATED TUI ON A CRT MESH *** CLICK A TAB *** ";
    let chars: Vec<char> = MSG.chars().collect();
    let len = chars.len();
    let offset = (frame_count / 3) as usize % len;
    let spans: Vec<Span<'static>> = (0..width as usize)
        .map(|i| {
            let c = chars[(offset + i) % len];
            let lit = (i as u32 + frame_count / 4) % 4 == 0;
            let style = if lit {
                Style::default()
                    .fg(RatatuiColor::Black)
                    .bg(RatatuiColor::Yellow)
                    .bold()
            } else {
                Style::default()
                    .fg(RatatuiColor::Yellow)
                    .bg(RatatuiColor::Black)
            };
            Span::styled(c.to_string(), style)
        })
        .collect();
    Line::from(spans)
}

/// Zero render-resource parameters: the plugin's `gpu_flush_system` owns
/// the GPU render + async copy + material touch, so this system only needs
/// the `Tui` and whatever app state it draws.
fn render_overlay_terminal(
    mut screens: Query<&mut Tui, With<OverlayScreen>>,
    app_state: Res<AppState>,
) {
    let Ok(mut term) = screens.single_mut() else {
        return;
    };
    term.draw_with_hits(|frame, hits| {
        let area = frame.area();

            // Simplified status display
            let status_lines = vec![
                Line::from(""),
                Line::from(vec![
                    checkbox_span(app_state.effects_enabled),
                    Span::raw(" FX  "),
                    checkbox_span(app_state.shadows_enabled),
                    Span::raw(" Shadow "),
                    Span::styled(
                        format!("{:.0} FPS", app_state.fps),
                        Style::default().fg(RatatuiColor::Cyan),
                    ),
                ]),
                Line::from(""),
                Line::from(vec![
                    Span::raw("Light:"),
                    Span::styled(
                        format!("{:.0}k", app_state.light_illuminance / 1000.0),
                        Style::default().fg(RatatuiColor::Yellow).bold(),
                    ),
                ]),
                Line::from(""),
                Line::from(vec![
                    radio_span(app_state.camera_mode == CameraMode::MouseFollow),
                    Span::raw(" Mouse Follow"),
                ]),
                Line::from(vec![
                    radio_span(app_state.camera_mode == CameraMode::Fixed),
                    Span::raw(" Fixed Front"),
                ]),
                Line::from(vec![
                    radio_span(app_state.camera_mode == CameraMode::Orbit),
                    Span::raw(" Auto Orbit"),
                ]),
                Line::from(""),
                Line::from(vec![
                    Span::raw("Clicks:"),
                    Span::styled(
                        app_state.button_click_count.to_string(),
                        Style::default().fg(RatatuiColor::Yellow).bold(),
                    ),
                ]),
                // Model credit
                Line::from(""),
                Line::from(""),
                Line::from(Span::styled(
                    "Original PC model by",
                    Style::default().fg(RatatuiColor::Gray),
                )),
                Line::from(Span::styled(
                    "CrazyDrPants, CC 4.0 Int'l",
                    Style::default().fg(RatatuiColor::Gray),
                )),
                Line::from(Span::styled(
                    " https://crazydrpants.itch.io/",
                    Style::default().fg(RatatuiColor::DarkGray),
                )),
                Line::from(Span::styled(
                    "           retro-crt-computer/",
                    Style::default().fg(RatatuiColor::DarkGray),
                )),
                Line::from(""),
                Line::from(Span::styled(
                    "Arranged by TTT",
                    Style::default().fg(RatatuiColor::Gray),
                )),
            ];

            let content = Paragraph::new(status_lines)
                .style(Style::default().bg(RatatuiColor::Rgb(15, 5, 15)))
                .block(
                    Block::bordered()
                        .title(Line::styled(
                            " Control Panel ",
                            Style::default().fg(RatatuiColor::Magenta).bold(),
                        ))
                        .border_style(Style::default().fg(RatatuiColor::Gray))
                        .style(Style::default().bg(RatatuiColor::Rgb(10, 5, 10))),
                );

            frame.render_widget(content, area);

            // Register checkbox / radio button hit regions right next to
            // the content that draws them - replaces the parallel Rect
            // bookkeeping this used to do in AppState.
            let inner_area = Block::bordered().inner(area);

            // CRT checkbox (row 0, start)
            hits.add(
                Hit::CrtCheckbox,
                ratatui::layout::Rect {
                    x: inner_area.x,
                    y: inner_area.y + 1,
                    width: 8, // width of "[X]CRT "
                    height: 1,
                },
            );

            // Shadows checkbox (row 0, middle)
            hits.add(
                Hit::ShadowsCheckbox,
                ratatui::layout::Rect {
                    x: inner_area.x + 8, // after "[X]CRT "
                    y: inner_area.y + 1,
                    width: 10, // width of "[X]Shadow"
                    height: 1,
                },
            );

            // Camera mode radio buttons (rows 5-7)
            hits.add(
                Hit::CameraRadio(0), // MouseFollow
                ratatui::layout::Rect {
                    x: inner_area.x,
                    y: inner_area.y + 5,
                    width: 18, // width of "(o)Mouse Follow"
                    height: 1,
                },
            );
            hits.add(
                Hit::CameraRadio(1), // Fixed
                ratatui::layout::Rect {
                    x: inner_area.x,
                    y: inner_area.y + 6,
                    width: 16, // width of "( )Fixed Front"
                    height: 1,
                },
            );
            hits.add(
                Hit::CameraRadio(2), // Orbit
                ratatui::layout::Rect {
                    x: inner_area.x,
                    y: inner_area.y + 7,
                    width: 16, // width of "( )Auto Orbit"
                    height: 1,
                },
            );
        });
}

fn update_camera_rotation(
    time: Res<Time>,
    app_state: Res<AppState>,
    mut camera_query: Query<&mut Transform, (With<OrbitCamera>, With<Camera3d>)>,
    windows: Query<&Window>,
) {
    let Ok(mut transform) = camera_query.single_mut() else {
        return;
    };
    let target = Vec3::new(0.0, 0.2, 0.0);

    transform.translation = match app_state.camera_mode {
        CameraMode::MouseFollow => {
            let Ok(window) = windows.single() else { return };
            let Some(cursor_pos) = window.cursor_position() else {
                return;
            };

            let mouse_x = (cursor_pos.x / window.width() - 0.5) * 2.0;
            let mouse_y = (cursor_pos.y / window.height() - 0.5) * 2.0;
            let max_angle = 30.0_f32.to_radians();

            let h_angle = mouse_x * max_angle;
            let v_angle = mouse_y * max_angle;
            let radius = 1.2;

            Vec3::new(
                h_angle.sin() * radius,
                0.5 + v_angle.sin() * 0.5,
                h_angle.cos() * radius,
            )
        }
        CameraMode::Fixed => Vec3::new(0.0, 0.5, 1.2),
        CameraMode::Orbit => {
            let angle = time.elapsed_secs() * 0.2;
            let radius = 1.2;
            Vec3::new(angle.cos() * radius, 0.5, angle.sin() * radius)
        }
    };

    transform.look_at(target, Vec3::Y);
}

fn update_directional_light(
    app_state: Res<AppState>,
    mut light_query: Query<&mut DirectionalLight, With<MainDirectionalLight>>,
) {
    if let Ok(mut light) = light_query.single_mut() {
        light.illuminance = app_state.light_illuminance;
        light.shadow_maps_enabled = app_state.shadows_enabled; // bevy 0.19 rename
                                                               // Debug output (first frame only)
        if app_state.frame_count == 1 {
            info!(
                "DirectionalLight updated: illuminance={}, shadows={}",
                app_state.light_illuminance, app_state.shadows_enabled
            );
        }
    } else if app_state.frame_count == 1 {
        info!("Failed to find MainDirectionalLight entity");
    }
}
