
use bevy::app::AppExit;
use bevy::asset::AssetMetaCheck;
#[cfg(not(target_arch = "wasm32"))]
use bevy::core_pipeline::oit::OrderIndependentTransparencySettings;
#[cfg(target_arch = "wasm32")]
use bevy::core_pipeline::tonemapping::Tonemapping;
use bevy::gltf::Gltf;
use bevy::pbr::{ExtendedMaterial, MaterialExtension};
use bevy::prelude::*;
use bevy::reflect::Reflect;
use bevy::render::render_resource::{AsBindGroup, ShaderType};
use bevy::shader::ShaderRef;
use bevy::window::WindowResolution;
use bevy::world_serialization::WorldAssetRoot;
use ratatui::prelude::*;
use ratatui::style::Color as RatatuiColor;
use ratatui::widgets::*;
use std::sync::Arc;
use tracing::info;
use unicode_width::UnicodeWidthStr;

use bevy_tui_texture::prelude::*;
use bevy_tui_texture::Font as TerminalFont;

pub fn main() {
    #[cfg(not(target_arch = "wasm32"))]
    build_app().run();
    // Native-only entry point - `wasm_main` in wasm_demo.rs is the wasm32
    // entry and calls `build_app` itself (it needs the wasm-only parameter
    // below), so this branch never runs there; `main` still needs a body
    // for wasm32's obligatory `fn main`, see wasm_demo.rs's own stub.
    #[cfg(target_arch = "wasm32")]
    build_app("assets").run();
}

/// Builds the fully configured scene `App` without running it.
pub(crate) fn build_app(#[cfg(target_arch = "wasm32")] asset_file_path: &str) -> App {
    let window_plugin = WindowPlugin {
        primary_window: Some(Window {
            title: "CRT Effect with Mesh3d".to_string(),
            resolution: WindowResolution::new(1024, 768),
            canvas: Some("#bevy".to_string()),
            fit_canvas_to_parent: true,
            prevent_default_event_handling: false,
            ..default()
        }),
        ..default()
    };

    let mut app = App::new();
    #[cfg(not(target_arch = "wasm32"))]
    app.add_plugins(DefaultPlugins.set(window_plugin).set(AssetPlugin {
        file_path: "examples/assets".into(),
        meta_check: AssetMetaCheck::Never,
        ..default()
    }));
    // `WgpuSettings::default()`'s `priority` is `Functionality`, which makes
    // `bevy_render::renderer::initialize_renderer` DISCARD the safe
    // `downlevel_webgl2_defaults()` limits it already computed and replace
    // them with `adapter.limits()`/`adapter.features()` queried directly
    // from the browser's WebGL2 adapter - together with an unconditional
    // `unsafe { wgpu::ExperimentalFeatures::enabled() }` on every device
    // request, this device descriptor has caused an unrecoverable hang
    // (not just a clean panic) on wasm32/WebGL2 in testing. Forcing
    // `WgpuSettingsPriority::WebGL2` keeps the conservative, known-safe
    // downlevel limits/features instead of trusting the adapter's raw
    // report.
    #[cfg(target_arch = "wasm32")]
    app.add_plugins(
        DefaultPlugins
            .set(window_plugin)
            .set(bevy::render::RenderPlugin {
                render_creation: bevy::render::settings::RenderCreation::Automatic(Box::new(
                    bevy::render::settings::WgpuSettings {
                        priority: bevy::render::settings::WgpuSettingsPriority::WebGL2,
                        ..default()
                    },
                )),
                ..default()
            })
            .set(AssetPlugin {
                file_path: asset_file_path.into(),
                meta_check: AssetMetaCheck::Never,
                ..default()
            }),
    );

    app.add_plugins(MaterialPlugin::<CrtMaterial>::default())
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
        .add_systems(Update, update_directional_light);

    app
}

#[derive(Clone, Copy, Debug, ShaderType, Reflect)]
pub(crate) struct CrtUniforms {
    effect_intensity: f32,
    time: f32,
    scan_line_intensity: f32,
    chromatic_aberration: f32,
}

#[derive(Asset, AsBindGroup, Clone, Reflect, Debug)]
pub(crate) struct CrtExtension {
    #[uniform(100)]
    pub uniforms: CrtUniforms,
}

impl MaterialExtension for CrtExtension {
    fn fragment_shader() -> ShaderRef {
        "shaders/crt_extended.wesl".into()
    }
}

pub(crate) type CrtMaterial = ExtendedMaterial<StandardMaterial, CrtExtension>;

#[derive(Clone, Copy, Debug, ShaderType, Reflect)]
struct BlurUniforms {
    effect_intensity: f32,
    time: f32,
    blur_radius: f32,
    blur_samples: f32,
}

#[derive(Asset, AsBindGroup, Clone, Debug, TypePath)]
struct BlurMaterial {
    #[uniform(0)]
    pub uniforms: BlurUniforms,

    #[texture(1)]
    #[sampler(2)]
    pub base_color_texture: Option<Handle<Image>>,
}

impl Material for BlurMaterial {
    fn vertex_shader() -> ShaderRef {
        "shaders/unlit_blur.wesl".into()
    }
    fn fragment_shader() -> ShaderRef {
        "shaders/unlit_blur.wesl".into()
    }

    fn alpha_mode(&self) -> AlphaMode {
        AlphaMode::Add
    }
    fn specialize(
        _pipeline: &bevy::pbr::MaterialPipeline,
        descriptor: &mut bevy::render::render_resource::RenderPipelineDescriptor,
        layout: &bevy::mesh::MeshVertexBufferLayoutRef,
        _key: bevy::pbr::MaterialPipelineKey<Self>,
    ) -> Result<(), bevy::render::render_resource::SpecializedMeshPipelineError> {
        let vertex_layout = layout.0.get_layout(&[
            Mesh::ATTRIBUTE_POSITION.at_shader_location(0),
            Mesh::ATTRIBUTE_COLOR.at_shader_location(1),
            Mesh::ATTRIBUTE_UV_0.at_shader_location(2),
        ])?;
        descriptor.vertex.buffers = vec![vertex_layout];

        if let Some(depth_stencil) = &mut descriptor.depth_stencil {
            depth_stencil.depth_write_enabled = Some(false);
        }

        if let Some(fragment) = &mut descriptor.fragment {
            for color_target in fragment.targets.iter_mut().flatten() {
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

        Ok(())
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
enum CameraMode {
    MouseFollow,
    Fixed,
    Orbit,
}

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

const TAB_TITLES: [&str; 3] = ["STATUS", "EFFECTS", "TABLE"];

/// Click regions across both terminals.
#[derive(Clone, Copy, Debug)]
enum Hit {
    Tab(u8),
    Button,
    TableRow(u8),
    CrtCheckbox,
    ShadowsCheckbox,
    CameraRadio(u8),
    /// The overlay panel's title bar - click to toggle `panel_collapsed`.
    PanelTitleBar,
    /// The `light_illuminance` slider's track (between and including its
    /// `[`/`]` brackets) - press-and-drag anywhere on it to set the value.
    LightSlider,
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
            Hit::PanelTitleBar => 0x07_00,
            Hit::LightSlider => 0x08_00,
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
            0x07_00 => Ok(Hit::PanelTitleBar),
            0x08_00 => Ok(Hit::LightSlider),
            _ => Err(()),
        }
    }
}

/// Overlay panel grid: 32 cols always; 21 rows expanded, 1 row (title bar
/// only) when collapsed by clicking the title bar.
const OVERLAY_COLS: u16 = 32;
const OVERLAY_ROWS: u16 = 21;
const OVERLAY_ROWS_COLLAPSED: u16 = 1;

/// `light_illuminance` slider geometry, in the overlay terminal's own
/// (0,0)-rooted grid coordinates - `render_overlay_terminal` always draws
/// this panel at `frame.area()` with a 1-cell `Block::bordered()` border,
/// so these are exact fixed columns/row, not read back from `HitRegions`.
const LIGHT_SLIDER_PREFIX: &str = "Light [";
const LIGHT_SLIDER_TRACK_COLS: u16 = 15;
const LIGHT_SLIDER_BORDER_LEFT: u16 = 1;
/// Column of the first character INSIDE the track's brackets.
const LIGHT_SLIDER_TRACK_X0: u16 =
    LIGHT_SLIDER_BORDER_LEFT + LIGHT_SLIDER_PREFIX.len() as u16;
/// Inner-area row the slider's `Line` is drawn on (see `status_lines` in
/// `render_overlay_terminal` - index 3, right after the FX/Shadow/FPS row).
const LIGHT_SLIDER_ROW: u16 = 3;
// A floor of 0 is deliberately NOT used here: log10(0) is undefined, and a slider that must represent "off"
// as an infinitely-small fraction of its travel is a bad slider anyway.
// 1.0 lux is near-total darkness for this scene's purposes (the emissive
// CRT screen stays fully lit regardless - see `claim_object2_screen` -
// only the DirectionalLight's own glass specular highlight actually goes
// to nothing at the low end).
const LIGHT_ILLUMINANCE_MIN: f32 = 1.0;
const LIGHT_ILLUMINANCE_MAX: f32 = 10_000.0;

/// Maps an absolute terminal column to a `light_illuminance` value using
/// the slider's fixed track geometry.
fn light_illuminance_from_col(col: u16) -> f32 {
    let offset = col.saturating_sub(LIGHT_SLIDER_TRACK_X0);
    let frac = (offset as f32 / (LIGHT_SLIDER_TRACK_COLS - 1) as f32).clamp(0.0, 1.0);
    let log_min = LIGHT_ILLUMINANCE_MIN.log10();
    let log_max = LIGHT_ILLUMINANCE_MAX.log10();
    10f32.powf(log_min + frac * (log_max - log_min))
}

/// Builds the `light_illuminance` slider's `Line`: `Light [████░░░] 2.0k`.
fn light_slider_line(value: f32) -> Vec<Span<'static>> {
    let log_min = LIGHT_ILLUMINANCE_MIN.log10();
    let log_max = LIGHT_ILLUMINANCE_MAX.log10();
    let frac = ((value.max(LIGHT_ILLUMINANCE_MIN).log10() - log_min) / (log_max - log_min))
        .clamp(0.0, 1.0);
    let track_cols = LIGHT_SLIDER_TRACK_COLS as usize;
    let filled = (frac * track_cols as f32).round() as usize;
    let bar = "█".repeat(filled) + &"░".repeat(track_cols - filled);
    let label = if value >= 1000.0 {
        format!("{:.1}k", value / 1000.0)
    } else {
        format!("{value:.0}")
    };
    vec![
        Span::raw(LIGHT_SLIDER_PREFIX),
        Span::styled(bar, Style::default().fg(RatatuiColor::Yellow)),
        Span::raw("] "),
        Span::styled(label, Style::default().fg(RatatuiColor::Yellow).bold()),
    ]
}

#[derive(Resource)]
struct AppState {
    effects_enabled: bool,
    frame_count: u32,
    fps: f32,
    button_clicked: bool,
    button_click_count: u32,
    light_illuminance: f32,
    /// Set while a press on `Hit::LightSlider` is held; subsequent
    /// `MouseEventKind::Moved`/`Drag(_)` events (even ones that stray off
    /// the slider's row, as long as the button stays down) keep updating
    /// `light_illuminance` until `MouseEventKind::Up`.
    light_slider_dragging: bool,
    shadows_enabled: bool,
    camera_mode: CameraMode,
    selected_tab: usize,
    table_state: TableState,
    panel_collapsed: bool,
}

impl Default for AppState {
    fn default() -> Self {
        Self {
            effects_enabled: true,
            frame_count: 0,
            fps: 0.0,
            button_clicked: false,
            button_click_count: 0,
            light_illuminance: 2000.0,
            light_slider_dragging: false,
            shadows_enabled: true,
            camera_mode: CameraMode::Orbit,
            selected_tab: 0,
            table_state: TableState::default(),
            panel_collapsed: false,
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

/// GLTF asset handle component (pub(crate): wasm_demo.rs's boot_status
/// polls its load state to surface a failed model download on the loading
/// overlay instead of stalling on "loading 3D model" forever)
#[derive(Component)]
pub(crate) struct GltfAsset(pub(crate) Handle<Gltf>);

/// Marker for the main CRT screen's `Tui` entity.
#[derive(Component)]
struct MainScreen;

/// Marker component to indicate GLTF has been spawned
#[derive(Component)]
struct GltfSpawned;

fn setup(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut standard_materials: ResMut<Assets<StandardMaterial>>,
    asset_server: Res<AssetServer>,
) {
    let font_data = include_bytes!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/examples/assets/fonts/fusion-pixel-10px-monospaced-ja.ttf"
    ));
    let font = TerminalFont::new(font_data).expect("Failed to load font");
    let fonts = Arc::new(Fonts::new(font, 16));

    let camera = commands
        .spawn((
            Camera3d::default(),
            Transform::from_xyz(0.0, 0.5, 1.2).looking_at(Vec3::new(0.0, 0.1, 0.0), Vec3::Y),
            Msaa::Off,
            OrbitCamera,
        ))
        .id();
    #[cfg(not(target_arch = "wasm32"))]
    commands
        .entity(camera)
        .insert(OrderIndependentTransparencySettings::default());
    #[cfg(target_arch = "wasm32")]
    commands
        .entity(camera)
        .insert(Tonemapping::KhronosPbrNeutral);

    let ground_material = StandardMaterial {
        base_color: bevy::color::Color::WHITE,
        metallic: 0.0,
        perceptual_roughness: 0.8,
        reflectance: 0.1,
        ..default()
    };
    let ground_material_handle = standard_materials.add(ground_material);

    let mut ground_mesh = Mesh::from(Circle::new(3.0));

    if let Some(positions) = ground_mesh
        .attribute(Mesh::ATTRIBUTE_POSITION)
        .and_then(|p| p.as_float3())
    {
        let mut colors = Vec::new();

        for position in positions {
            let distance = (position[0] * position[0] + position[2] * position[2]).sqrt();
            let normalized_distance = (distance / 3.0).clamp(0.0, 1.0);

            let brightness = 0.22 - (normalized_distance * 0.20);
            colors.push([brightness, brightness, brightness, 1.0]);
        }

        ground_mesh.insert_attribute(Mesh::ATTRIBUTE_COLOR, colors);
    }

    let ground_mesh_handle = meshes.add(ground_mesh);

    commands.spawn((
        Mesh3d(ground_mesh_handle),
        MeshMaterial3d(ground_material_handle),
        Transform::from_rotation(Quat::from_rotation_x(-std::f32::consts::FRAC_PI_2)),
        GroundMesh,
    ));

    commands.spawn((
        DirectionalLight {
            illuminance: 2000.0,
            shadow_maps_enabled: true,
            ..default()
        },
        Transform::from_rotation(Quat::from_euler(EulerRot::XYZ, -0.5, -0.5, 0.0)),
        MainDirectionalLight,
    ));

    commands.insert_resource(ClearColor(bevy::color::Color::srgb(0.015, 0.015, 0.025)));

    let gltf_handle: Handle<Gltf> = asset_server.load("models/retro_crt.glb");
    commands.spawn((GltfAsset(gltf_handle), GltfModel));

    commands.spawn((
        TuiRequest::ui(OVERLAY_COLS, OVERLAY_ROWS, fonts.clone()),
        Node {
            position_type: PositionType::Absolute,
            right: Val::Px(20.0),
            top: Val::Px(20.0),
            ..default()
        },
        OverlayScreen,
    ));

    commands.spawn((
        TuiRequest::headless(32, 24, fonts).with_config(TerminalConfig {
            initial_draw: Some(Box::new(|frame| {
                let area = frame.area();

                let clear =
                    Block::default().style(Style::default().bg(RatatuiColor::Rgb(10, 10, 30)));
                frame.render_widget(clear, area);

                let title = Paragraph::new("Shader with ExtendedMaterial - Loading...")
                    .style(
                        Style::default()
                            .fg(RatatuiColor::Green)
                            .bg(RatatuiColor::DarkGray)
                            .bold(),
                    )
                    .alignment(Alignment::Center)
                    .block(
                        Block::bordered().border_style(Style::default().fg(RatatuiColor::White)),
                    );
                frame.render_widget(title, area);
            })),
            ..default()
        }),
        MainScreen,
    ));
    commands.insert_resource(AppState::default());
}

fn spawn_gltf_scene_simple(
    mut commands: Commands,
    gltf_query: Query<(Entity, &GltfAsset), (With<GltfModel>, Without<GltfSpawned>)>,
    gltf_assets: Res<Assets<Gltf>>,
) {
    if gltf_query.is_empty() {
        return;
    }

    for (gltf_entity, gltf_asset) in gltf_query.iter() {
        let Some(gltf) = gltf_assets.get(&gltf_asset.0) else {
            continue;
        };

        info!(
            "GLTF asset loaded! Found {} scenes, spawning...",
            gltf.scenes.len()
        );

        if let Some(scene) = gltf.scenes.first() {
            commands.spawn((
                WorldAssetRoot(scene.clone()),
                Transform::from_rotation(Quat::from_rotation_y(-std::f32::consts::FRAC_PI_2)),
            ));
            info!("GLTF scene spawned successfully with 90-degree clockwise rotation");
        }

        commands.entity(gltf_entity).insert(GltfSpawned);
    }
}

/// Claims Object_2 (Monitor Glass) by inserting `AttachTerminal`, pointing
/// it at the main screen's `Tui`.
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
        return;
    };

    for (entity, name, standard_material_handle) in &to_claim {
        if name.as_str() != "Object_2.Monitor_Glass" {
            continue;
        }
        let Some(standard_material) = standard_materials.get(standard_material_handle) else {
            continue;
        };

        info!(
            "Found Object_2 (Monitor Glass): '{}' - attaching Tui",
            name.as_str()
        );

        let double_sided = standard_material.double_sided;
        let cull_mode = standard_material.cull_mode;

        commands.entity(entity).insert(AttachTerminal {
            terminal: main_screen,
            material: AttachMaterial::custom(move |image| CrtMaterial {
                base: StandardMaterial {
                    // Self-illuminating screen: the terminal texture goes
                    // through the EMISSIVE channel, not base_color_texture.
                    // bevy_pbr's apply_pbr_lighting adds emissive_light
                    // AFTER the light-dependent diffuse/specular terms
                    // (bevy_pbr's `pbr_functions`:
                    // `... + emissive_light`, no light-intensity factor
                    // anywhere in that term) - so the screen content stays
                    // fully visible even at `light_illuminance = 0`, same
                    // as the additive Monitor_Reflection overlay
                    // (unlit_blur.wesl, a fully custom Material that never
                    // samples scene lights at all - see
                    // `claim_monitor_reflection`).
                    //
                    // base_color stays black (no diffuse response to
                    // scene light) while metallic/roughness/reflectance
                    // below are kept so the DirectionalLight can still add
                    // a dielectric glass specular highlight ON TOP of the
                    // emissive content - that highlight is the one part of
                    // this material that's legitimately light-dependent,
                    // since it represents light bouncing off the glass,
                    // not the screen's own picture.
                    base_color: bevy::color::Color::BLACK,
                    emissive_texture: Some(image),
                    emissive: bevy::color::Color::WHITE.to_linear(),
                    unlit: false,
                    alpha_mode: AlphaMode::Opaque,
                    double_sided,
                    cull_mode,
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
/// for click hit-testing with the real screen).
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

        info!(
            "Found Monitor_Reflection: '{}' - attaching BlurMaterial",
            name_str
        );

        let blur_material_handle = blur_materials.add(BlurMaterial {
            uniforms: BlurUniforms {
                effect_intensity: 1.0,
                time: 0.0,
                blur_radius: 3.0,
                blur_samples: 5.0,
            },
            base_color_texture: Some(main_tui.image_handle().clone()),
        });

        commands
            .entity(entity)
            .remove::<MeshMaterial3d<StandardMaterial>>()
            .insert(MeshMaterial3d(blur_material_handle))
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
        return;
    };
    let overlay = overlay_query.single().ok();

    for event in events.read() {
        if event.target == main_entity {
            if let InputEvent::Mouse(m) = &event.input
                && matches!(m.kind, MouseEventKind::Down(_)) {
                    info!(
                        "Object_2 mouse click at col={}, row={}",
                        m.column, m.row
                    );

                    match main_tui.hit_regions().hit_at::<Hit>((m.column, m.row)) {
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
        } else if let Some((_, overlay_tui)) = overlay.filter(|(entity, _)| *entity == event.target)
        {
            match &event.input {
                InputEvent::Mouse(m) if matches!(m.kind, MouseEventKind::Down(_)) => {
                    info!(
                        "Overlay terminal mouse click at col={}, row={}",
                        m.column, m.row
                    );

                    match overlay_tui.hit_regions().hit_at::<Hit>((m.column, m.row)) {
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
                        Some(Hit::PanelTitleBar) => {
                            app_state.panel_collapsed = !app_state.panel_collapsed;
                            info!(
                                "🎯 Panel {}",
                                if app_state.panel_collapsed {
                                    "collapsed"
                                } else {
                                    "expanded"
                                }
                            );
                        }
                        Some(Hit::LightSlider) => {
                            app_state.light_slider_dragging = true;
                            app_state.light_illuminance = light_illuminance_from_col(m.column);
                            info!("🎯 Light: {:.0}", app_state.light_illuminance);
                        }
                        _ => {}
                    }
                }
                InputEvent::Mouse(m)
                    if app_state.light_slider_dragging
                        && matches!(m.kind, MouseEventKind::Moved | MouseEventKind::Drag(_)) =>
                {
                    app_state.light_illuminance = light_illuminance_from_col(m.column);
                }
                InputEvent::Mouse(m) if matches!(m.kind, MouseEventKind::Up(_)) => {
                    app_state.light_slider_dragging = false;
                }
                _ => {}
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
    for material_handle in &object2_query {
        if let Some(mut material) = crt_materials.get_mut(&material_handle.0) {
            material.extension.uniforms.effect_intensity =
                if app_state.effects_enabled { 1.0 } else { 0.0 };
            material.extension.uniforms.time = time.elapsed_secs();
        }
    }

}

fn update_blur_uniforms(
    mut blur_materials: ResMut<Assets<BlurMaterial>>,
    reflection_query: Query<&MeshMaterial3d<BlurMaterial>>,
    app_state: Res<AppState>,
    time: Res<Time>,
) {
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

    let dt = time.delta_secs();
    if dt > 0.0 {
        let instant_fps = 1.0 / dt;
        app_state.fps = if app_state.fps <= 0.0 {
            instant_fps
        } else {
            app_state.fps * 0.9 + instant_fps * 0.1
        };
    }

    if app_state.button_clicked && app_state.frame_count.is_multiple_of(60) {
        app_state.button_clicked = false;
    }

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
                    Paragraph::new(label).style(Style::default().fg(RatatuiColor::Black).bg(color)),
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
            Constraint::Length(2),
            Constraint::Length(1),
            Constraint::Length(1),
            Constraint::Length(1),
            Constraint::Length(1),
            Constraint::Length(15),
            Constraint::Length(1),
        ])
        .split(interior);
        let (banner_area, rule_area, tabs_area, content_area, marquee_area) = (
            sections[0],
            sections[1],
            sections[3],
            sections[5],
            sections[6],
        );

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
            frame.render_widget(
                Block::default().style(Style::default().bg(RatatuiColor::Black).fg(*color)),
                band,
            );
        }
        frame.render_widget(RatatuiLogo::small(), logo_rect);

        frame.render_widget(
            Paragraph::new("═".repeat(rule_area.width as usize))
                .style(Style::default().fg(RatatuiColor::DarkGray)),
            rule_area,
        );

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

        match app_state.selected_tab {
            0 => render_status_tab(frame, content_area, &app_state, hits),
            1 => render_effects_tab(frame, content_area),
            _ => render_table_tab(frame, content_area, &mut app_state, hits),
        }

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
        Constraint::Length(3),
        Constraint::Length(1),
        Constraint::Length(1),
        Constraint::Length(1),
        Constraint::Length(1),
        Constraint::Length(1),
        Constraint::Length(1),
        Constraint::Length(1),
        Constraint::Min(0),
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
        Constraint::Length(1),
        Constraint::Length(1),
        Constraint::Length(1),
        Constraint::Length(8),
        Constraint::Min(0),
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
            let lit = (i as u32 + frame_count / 4).is_multiple_of(4);
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

    let target_rows = if app_state.panel_collapsed {
        OVERLAY_ROWS_COLLAPSED
    } else {
        OVERLAY_ROWS
    };
    term.request_resize(OVERLAY_COLS, target_rows);

    term.draw_with_hits(|frame, hits| {
        let area = frame.area();

        hits.add(
            Hit::PanelTitleBar,
            ratatui::layout::Rect {
                x: area.x,
                y: area.y,
                width: area.width,
                height: 1,
            },
        );

        if app_state.panel_collapsed {
            frame.render_widget(
                Paragraph::new(Line::styled(
                    "  > bevy_tui_texture Demo",
                    Style::default().fg(RatatuiColor::Magenta).bold(),
                ))
                .alignment(Alignment::Left)
                .style(Style::default().bg(RatatuiColor::Rgb(10, 5, 10))),
                area,
            );
            return;
        }

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
            Line::from(light_slider_line(app_state.light_illuminance)),
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
                        " v bevy_tui_texture Demo ",
                        Style::default().fg(RatatuiColor::Magenta).bold(),
                    ))
                    .border_style(Style::default().fg(RatatuiColor::Gray))
                    .style(Style::default().bg(RatatuiColor::Rgb(10, 5, 10))),
            );

        frame.render_widget(content, area);

        let inner_area = Block::bordered().inner(area);

        hits.add(
            Hit::CrtCheckbox,
            ratatui::layout::Rect {
                x: inner_area.x,
                y: inner_area.y + 1,
                width: 8,
                height: 1,
            },
        );

        hits.add(
            Hit::ShadowsCheckbox,
            ratatui::layout::Rect {
                x: inner_area.x + 8,
                y: inner_area.y + 1,
                width: 10,
                height: 1,
            },
        );

        hits.add(
            Hit::LightSlider,
            ratatui::layout::Rect {
                x: inner_area.x + LIGHT_SLIDER_PREFIX.len() as u16 - 1,
                y: inner_area.y + LIGHT_SLIDER_ROW,
                width: LIGHT_SLIDER_TRACK_COLS + 2,
                height: 1,
            },
        );

        hits.add(
            Hit::CameraRadio(0),
            ratatui::layout::Rect {
                x: inner_area.x,
                y: inner_area.y + 5,
                width: 18,
                height: 1,
            },
        );
        hits.add(
            Hit::CameraRadio(1),
            ratatui::layout::Rect {
                x: inner_area.x,
                y: inner_area.y + 6,
                width: 16,
                height: 1,
            },
        );
        hits.add(
            Hit::CameraRadio(2),
            ratatui::layout::Rect {
                x: inner_area.x,
                y: inner_area.y + 7,
                width: 16,
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
    cursor: Res<CursorPosition>,
) {
    let Ok(mut transform) = camera_query.single_mut() else {
        return;
    };
    let target = Vec3::new(0.0, 0.2, 0.0);

    transform.translation = match app_state.camera_mode {
        CameraMode::MouseFollow => {
            let Ok(window) = windows.single() else { return };
            // `window.cursor_position()` is None on touch devices - no
            // mouse cursor exists there, so Mouse Follow would never move.
            // `CursorPosition` (this crate's own tracked resource) already
            // falls back to the first active touch's position on such
            // devices (see its doc comment / `update_cursor_position_system`),
            // so a touch-and-drag on a phone drives this the same way a
            // mouse move does on desktop.
            let Some(cursor_pos) = cursor.position else {
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
        light.shadow_maps_enabled = app_state.shadows_enabled;
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
