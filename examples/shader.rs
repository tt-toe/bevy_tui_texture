// ExtendedMaterial CRT Example - Custom Fragment Shader with CRT Effects
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
// Press ESC to quit

use bevy::app::AppExit;
use bevy::pbr::{ExtendedMaterial, MaterialExtension};
use bevy::prelude::*;
use bevy::reflect::Reflect;
use bevy::render::render_resource::{AsBindGroup, ShaderType};
use bevy::render::renderer::{RenderDevice, RenderQueue};
use bevy::shader::ShaderRef;
use bevy::window::WindowResolution;
use ratatui::prelude::*;
use ratatui::style::Color as RatatuiColor;
use ratatui::widgets::*;
use std::sync::Arc;
use tracing::info;

use bevy_tui_texture::Font as TerminalFont;
use bevy_tui_texture::prelude::*;

fn main() {
    App::new()
        .add_plugins(DefaultPlugins.set(WindowPlugin {
            primary_window: Some(Window {
                title: "ExtendedMaterial CRT Effect - Terminal Texture".to_string(),
                resolution: WindowResolution::new(1024, 768),
                ..default()
            }),
            ..default()
        }))
        .add_plugins(MaterialPlugin::<CrtMaterial>::default())
        .add_plugins(TerminalPlugin::default())
        .add_systems(Startup, setup)
        .add_systems(Update, handle_input.in_set(TerminalSystemSet::UserUpdate))
        .add_systems(Update, render_terminal.in_set(TerminalSystemSet::Render))
        .add_systems(Update, update_crt_uniforms)
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

#[derive(Resource)]
struct TerminalState {
    texture: TerminalTexture,
    material_handle: Handle<CrtMaterial>,
}

#[derive(Resource)]
struct AppState {
    effects_enabled: bool,
    frame_count: u32,
}

impl Default for AppState {
    fn default() -> Self {
        Self {
            effects_enabled: true,
            frame_count: 0,
        }
    }
}

#[derive(Component)]
struct MainTerminal;

fn setup(
    mut commands: Commands,
    render_device: Res<RenderDevice>,
    render_queue: Res<RenderQueue>,
    mut images: ResMut<Assets<Image>>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<CrtMaterial>>,
) {
    // Load font
    let font_data = include_bytes!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/assets/fonts/fusion-pixel-10px-monospaced-ja.ttf"
    ));
    let font = TerminalFont::new(font_data).expect("Failed to load font");
    let fonts = Arc::new(Fonts::new(font, 16));

    // Create terminal texture
    let mut texture = TerminalTexture::create(
        80,
        30,
        fonts,
        true,
        &render_device,
        &render_queue,
        &mut images,
    )
    .expect("Failed to create terminal");

    // Create ExtendedMaterial (KEY DIFFERENCE from standard material)
    let material = CrtMaterial {
        base: StandardMaterial {
            base_color: bevy::color::Color::WHITE, // CRITICAL: White to show texture colors accurately
            base_color_texture: Some(texture.image_handle.clone()),
            unlit: false, // Must be false for pbr_input_from_standard_material to work
            alpha_mode: AlphaMode::Opaque,
            double_sided: true,
            cull_mode: None,
            ..default()
        },
        extension: CrtExtension {
            uniforms: CrtUniforms {
                effect_intensity: 1.0, // Start with effects enabled
                time: 0.0,
                scan_line_intensity: 0.1,    // Subtle scan lines
                chromatic_aberration: 0.002, // Slight color separation
            },
        },
    };
    let material_handle = materials.add(material);

    // Create mesh (Y-up plane facing camera)
    let mesh = Mesh::from(Plane3d::new(Vec3::Y, Vec2::new(4.0, 3.0)));
    let mesh_handle = meshes.add(mesh);

    // Spawn terminal entity
    commands.spawn((
        Mesh3d(mesh_handle),
        MeshMaterial3d(material_handle.clone()),
        Transform::from_xyz(0.0, 0.0, 0.0),
        MainTerminal,
        texture.dimensions(),
        TerminalInput::default(),
        TerminalComponent,
    ));

    // Camera
    commands.spawn((
        Camera3d::default(),
        Transform::from_xyz(0.0, 10.0, 5.0).looking_at(Vec3::ZERO, Vec3::Y),
    ));

    // Directional light
    commands.spawn((
        DirectionalLight {
            illuminance: 5000.0,
            shadows_enabled: false,
            ..default()
        },
        Transform::from_rotation(Quat::from_euler(EulerRot::XYZ, -0.5, -0.5, 0.0)),
    ));

    // Initial synchronous render (prevents first-frame black texture)
    {
        use bevy_tui_texture::bevy_plugin::update_terminal_texture;

        let _ = texture.terminal.draw(|frame| {
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

        let texture_view = texture
            .texture
            .create_view(&wgpu::TextureViewDescriptor::default());
        texture.terminal.backend_mut().render_to_texture(
            render_device.wgpu_device(),
            render_queue.0.as_ref(),
            &texture_view,
        );

        // Synchronous copy to populate Image immediately
        update_terminal_texture(
            &texture.texture,
            &texture.image_handle,
            texture.width,
            texture.height,
            &render_device,
            &render_queue,
            &mut images,
        );
    }

    // Insert resources
    commands.insert_resource(TerminalState {
        texture,
        material_handle,
    });
    commands.insert_resource(AppState::default());
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
}

fn update_crt_uniforms(
    mut materials: ResMut<Assets<CrtMaterial>>,
    terminal_state: Res<TerminalState>,
    app_state: Res<AppState>,
    time: Res<Time>,
) {
    if let Some(material) = materials.get_mut(&terminal_state.material_handle) {
        material.extension.uniforms.effect_intensity =
            if app_state.effects_enabled { 1.0 } else { 0.0 };
        material.extension.uniforms.time = time.elapsed_secs();
    }
}

fn render_terminal(
    mut terminal_state: ResMut<TerminalState>,
    mut app_state: ResMut<AppState>,
    render_device: Res<RenderDevice>,
    render_queue: Res<RenderQueue>,
    mut images: ResMut<Assets<Image>>,
) {
    app_state.frame_count += 1;

    // Use async update for Bevy's material system
    terminal_state
        .texture
        .update(&render_device, &render_queue, &mut images, |frame| {
            let area = frame.area();

            // Title
            let title = Paragraph::new("Shader with ExtendedMaterial - Terminal Texture")
                .style(
                    Style::default()
                        .fg(RatatuiColor::Green)
                        .bg(RatatuiColor::DarkGray)
                        .bold(),
                )
                .alignment(Alignment::Center)
                .block(Block::bordered().border_style(Style::default().fg(RatatuiColor::White)));

            // Status
            let status = format!(
                "Frame: {} | SPACE: Toggle Effects | ESC: Quit",
                app_state.frame_count
            );
            let status_widget = Paragraph::new(status)
                .style(
                    Style::default()
                        .fg(RatatuiColor::Yellow)
                        .bg(RatatuiColor::Rgb(40, 40, 0)),
                )
                .alignment(Alignment::Center);

            // Demo content
            let content_lines = vec![
                Line::from(""),
                Line::from(Span::styled(
                    "ExtendedMaterial CRT Effects",
                    Style::default().fg(RatatuiColor::Green).bold(),
                )),
                Line::from(""),
                Line::from("This example demonstrates custom fragment shaders using"),
                Line::from("ExtendedMaterial to avoid binding conflicts."),
                Line::from(""),
                Line::from(Span::styled(
                    "CRT Effects:",
                    Style::default().fg(RatatuiColor::Yellow).bold(),
                )),
                Line::from(""),
                Line::from(vec![
                    Span::raw("  • "),
                    Span::styled("Scan lines", Style::default().fg(RatatuiColor::LightRed)),
                    Span::raw(" - Animated horizontal phosphor lines"),
                ]),
                Line::from(vec![
                    Span::raw("  • "),
                    Span::styled("Vignette", Style::default().fg(RatatuiColor::LightGreen)),
                    Span::raw(" - Darkened screen edges"),
                ]),
                Line::from(vec![
                    Span::raw("  • "),
                    Span::styled(
                        "Phosphor glow",
                        Style::default().fg(RatatuiColor::LightBlue),
                    ),
                    Span::raw(" - Subtle gamma adjustment"),
                ]),
                Line::from(vec![
                    Span::raw("  • "),
                    Span::styled(
                        "Color shift",
                        Style::default().fg(RatatuiColor::LightMagenta),
                    ),
                    Span::raw(" - Chromatic aberration approximation"),
                ]),
                Line::from(""),
                Line::from(Span::styled(
                    "Color Test:",
                    Style::default().fg(RatatuiColor::Cyan).bold(),
                )),
                Line::from(""),
                Line::from(vec![
                    Span::styled(
                        "█████",
                        Style::default()
                            .fg(RatatuiColor::Red)
                            .bg(RatatuiColor::Rgb(40, 0, 0)),
                    ),
                    Span::raw(" "),
                    Span::styled(
                        "█████",
                        Style::default()
                            .fg(RatatuiColor::Green)
                            .bg(RatatuiColor::Rgb(0, 40, 0)),
                    ),
                    Span::raw(" "),
                    Span::styled(
                        "█████",
                        Style::default()
                            .fg(RatatuiColor::Blue)
                            .bg(RatatuiColor::Rgb(0, 0, 40)),
                    ),
                    Span::raw(" "),
                    Span::styled(
                        "█████",
                        Style::default()
                            .fg(RatatuiColor::Yellow)
                            .bg(RatatuiColor::Rgb(40, 40, 0)),
                    ),
                ]),
                Line::from(""),
            ];

            let content = Paragraph::new(content_lines)
                .style(Style::default().bg(RatatuiColor::Rgb(20, 20, 40)))
                .block(
                    Block::bordered()
                        .title("Demo Content")
                        .border_style(Style::default().fg(RatatuiColor::Gray))
                        .style(Style::default().bg(RatatuiColor::Rgb(20, 20, 40))),
                );

            // Layout
            let chunks = Layout::default()
                .direction(Direction::Vertical)
                .constraints([
                    Constraint::Length(3), // Title
                    Constraint::Min(0),    // Content
                    Constraint::Length(1), // Status
                ])
                .split(area);

            frame.render_widget(title, chunks[0]);
            frame.render_widget(content, chunks[1]);
            frame.render_widget(status_widget, chunks[2]);
        });
}
