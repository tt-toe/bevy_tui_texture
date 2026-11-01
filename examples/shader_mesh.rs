use bevy::app::AppExit;
use bevy::pbr::{ExtendedMaterial, MaterialExtension};
use bevy::prelude::*;
use bevy::reflect::Reflect;
use bevy::render::render_resource::{AsBindGroup, ShaderType};
use bevy::shader::ShaderRef;
use bevy::window::WindowResolution;
use ratatui::prelude::*;
use ratatui::style::Color as RatatuiColor;
use ratatui::widgets::*;
use std::sync::Arc;
use tracing::info;

use bevy_tui_texture::prelude::*;
use bevy_tui_texture::Font as TerminalFont;

fn main() {
    App::new()
        .add_plugins(
            DefaultPlugins
                .set(WindowPlugin {
                    primary_window: Some(Window {
                        title: "ExtendedMaterial CRT Effect with Mesh3d".to_string(),
                        resolution: WindowResolution::new(1024, 768),
                        ..default()
                    }),
                    ..default()
                })
                .set(AssetPlugin {
                    file_path: "examples/assets".into(),
                    ..default()
                }),
        )
        .add_plugins(MaterialPlugin::<CrtMaterial>::default())
        .add_plugins(TerminalPlugin::default())
        .add_systems(Startup, setup)
        .add_systems(Update, handle_input.in_set(TerminalSystemSet::UserUpdate))
        .add_systems(Update, render_terminal.in_set(TerminalSystemSet::Render))
        .add_systems(Update, update_crt_uniforms)
        .run();
}

#[derive(Clone, Copy, Debug, ShaderType, Reflect)]
struct CrtUniforms {
    effect_intensity: f32,
    time: f32,
    scan_line_intensity: f32,
    chromatic_aberration: f32,
}

#[derive(Asset, AsBindGroup, Clone, Reflect, Debug)]
struct CrtExtension {
    #[uniform(100)]
    pub uniforms: CrtUniforms,
}

impl MaterialExtension for CrtExtension {
    fn fragment_shader() -> ShaderRef {
        "shaders/crt_extended.wesl".into()
    }
}

type CrtMaterial = ExtendedMaterial<StandardMaterial, CrtExtension>;

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
    mut images: ResMut<Assets<Image>>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<CrtMaterial>>,
) {
    let font_data = include_bytes!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/examples/assets/fonts/fusion-pixel-10px-monospaced-ja.ttf"
    ));
    let font = TerminalFont::new(font_data).expect("Failed to load font");
    let fonts = Arc::new(Fonts::new(font, 16));

    let texture_state = TerminalTexture::create(80, 30, fonts, true, false, [0, 0, 0, 255], &mut images)
        .expect("Failed to create terminal");

    let image_handle = texture_state.image_handle();
    let dimensions = texture_state.dimensions();
    let mut tui = Tui::from_texture_state(texture_state);

    tui.draw(|frame| {
        let area = frame.area();

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

    let material = CrtMaterial {
        base: StandardMaterial {
            base_color: bevy::color::Color::WHITE,
            base_color_texture: Some(image_handle),
            unlit: false, // Must be false for pbr_input_from_standard_material to work
            alpha_mode: AlphaMode::Opaque,
            double_sided: true,
            cull_mode: None,
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
    };
    let material_handle = materials.add(material);

    let mesh = Mesh::from(Plane3d::new(Vec3::Y, Vec2::new(4.0, 3.0)));
    let mesh_handle = meshes.add(mesh);

    let terminal_entity = commands
        .spawn((
            Mesh3d(mesh_handle),
            MeshMaterial3d(material_handle),
            Transform::from_xyz(0.0, 0.0, 0.0),
            MainTerminal,
            dimensions,
            TerminalInput::default(),
        ))
        .id();
    commands.entity(terminal_entity).insert((
        tui,
        TuiSurface {
            tui: terminal_entity,
        },
    ));

    commands.spawn((
        Camera3d::default(),
        Transform::from_xyz(0.0, 10.0, 5.0).looking_at(Vec3::ZERO, Vec3::Y),
    ));

    commands.spawn((
        DirectionalLight {
            illuminance: 5000.0,
            shadow_maps_enabled: false,
            ..default()
        },
        Transform::from_rotation(Quat::from_euler(EulerRot::XYZ, -0.5, -0.5, 0.0)),
    ));

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
    terminal: Query<&MeshMaterial3d<CrtMaterial>, With<MainTerminal>>,
    app_state: Res<AppState>,
    time: Res<Time>,
) {
    let Ok(material_handle) = terminal.single() else {
        return;
    };
    if let Some(mut material) = materials.get_mut(&material_handle.0) {
        material.extension.uniforms.effect_intensity =
            if app_state.effects_enabled { 1.0 } else { 0.0 };
        material.extension.uniforms.time = time.elapsed_secs();
    }
}

/// Zero render-resource parameters: `gpu_flush_system` (registered by
/// `TerminalPlugin`) renders into the library-owned texture, and a
/// render-world system copies it straight into `CrtMaterial`'s bind group -
/// no plugin registration or per-frame touching needed for this custom
/// material type.
fn render_terminal(mut screens: Query<&mut Tui, With<MainTerminal>>, mut app_state: ResMut<AppState>) {
    app_state.frame_count += 1;

    let Ok(mut term) = screens.single_mut() else {
        return;
    };
    term.draw(|frame| {
            let area = frame.area();

            let title = Paragraph::new("Shader with ExtendedMaterial - Terminal Texture")
                .style(
                    Style::default()
                        .fg(RatatuiColor::Green)
                        .bg(RatatuiColor::DarkGray)
                        .bold(),
                )
                .alignment(Alignment::Center)
                .block(Block::bordered().border_style(Style::default().fg(RatatuiColor::White)));

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

            let chunks = Layout::default()
                .direction(Direction::Vertical)
                .constraints([
                    Constraint::Length(3),
                    Constraint::Min(0),
                    Constraint::Length(1),
                ])
                .split(area);

            frame.render_widget(title, chunks[0]);
            frame.render_widget(content, chunks[1]);
            frame.render_widget(status_widget, chunks[2]);
        });
}
