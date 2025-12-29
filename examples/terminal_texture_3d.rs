// TerminalTexture 3D Example
//
// Demonstrates manual 3D terminal setup using only TerminalTexture API.
// This gives you full control over mesh, material, and entity creation.
//
// Use this when you need:
// - Custom mesh shapes (not just planes)
// - Custom material properties
// - Fine-grained control over transforms
// - Integration with existing 3D scenes
// - Multiple terminals with different configurations
//
// For simpler use cases, use SimpleTerminal3D instead.

use std::sync::Arc;

use bevy::pbr::StandardMaterial;
use bevy::prelude::*;
use bevy::render::renderer::{RenderDevice, RenderQueue};
use bevy::window::WindowResolution;
use ratatui::prelude::*;
use ratatui::style::Color as RatatuiColor;
use ratatui::widgets::*;

use bevy_tui_texture::Font as TerminalFont;
use bevy_tui_texture::prelude::*;

fn main() {
    App::new()
        .add_plugins(DefaultPlugins.set(WindowPlugin {
            primary_window: Some(Window {
                title: "3D TerminalTexture - Manual Setup Example".to_string(),
                resolution: WindowResolution::new(1024, 768),
                ..default()
            }),
            ..default()
        }))
        .add_plugins(TerminalPlugin::default())
        .add_systems(Startup, setup)
        .add_systems(Update, handle_input.in_set(TerminalSystemSet::UserUpdate))
        .add_systems(Update, render_terminal.in_set(TerminalSystemSet::Render))
        .add_systems(Update, rotate_terminal)
        .run();
}

// Resource holding our manually created terminal texture
#[derive(Resource)]
struct MyTerminal {
    texture: TerminalTexture,
    material_handle: Handle<StandardMaterial>, // We keep this to update it
}

// App state
#[derive(Resource, Default)]
struct AppState {
    counter: usize,
    last_key: Option<String>,
    mouse_pos: Option<(u16, u16)>,
}

// Marker component for our terminal entity
#[derive(Component)]
struct MainTerminal;

fn setup(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    render_device: Res<RenderDevice>,
    render_queue: Res<RenderQueue>,
    mut images: ResMut<Assets<Image>>,
) {
    info!("=== Manual 3D Terminal Setup with TerminalTexture ===");

    // Step 1: Load font
    let font_data = include_bytes!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/assets/fonts/Mplus1Code-Regular.ttf"
    ));
    let font = TerminalFont::new(font_data).expect("Failed to load font");
    let fonts = Arc::new(Fonts::new(font, 16));

    // Step 2: Create terminal texture (just the texture + terminal, no entity)
    let texture = TerminalTexture::create(
        80,
        25, // 80x25 terminal
        fonts,
        true, // Enable programmatic glyphs
        &render_device,
        &render_queue,
        &mut images,
    )
    .expect("Failed to create terminal texture");

    info!(
        "Created TerminalTexture: {}x{} pixels",
        texture.width, texture.height
    );

    // Step 3: Create 3D mesh
    // You have full control over the mesh shape and size!
    let mesh = meshes.add(
        Plane3d::default()
            .mesh()
            .size(texture.width as f32, texture.height as f32),
    );

    // Step 4: Create material with the terminal texture
    // You have full control over material properties!
    let material = materials.add(StandardMaterial {
        base_color_texture: Some(texture.image_handle()),
        unlit: true, // Disable lighting for terminal display
        alpha_mode: AlphaMode::Blend,
        // You could customize:
        // - emissive: Color for glowing effect
        // - metallic/roughness: Material properties
        // - double_sided: true for visibility from both sides
        // etc.
        ..default()
    });

    // Step 5: Spawn 3D camera
    commands.spawn((
        Camera3d::default(),
        Transform::from_xyz(0.0, 0.0, 800.0).looking_at(Vec3::ZERO, Vec3::Y),
    ));

    // Step 6: Add ambient light
    commands.insert_resource(AmbientLight {
        color: bevy::color::Color::WHITE,
        brightness: 1.0,
        affects_lightmapped_meshes: true,
    });

    // Step 7: Manually spawn the terminal entity with custom components
    // This is where you have full control!
    commands.spawn((
        // Standard Bevy 3D components
        Mesh3d(mesh),
        MeshMaterial3d(material.clone()),
        Transform {
            translation: Vec3::ZERO,
            rotation: Quat::from_rotation_x(-std::f32::consts::FRAC_PI_2), // Face camera
            scale: Vec3::ONE,
            // You could customize: rotation, scale, translation
        },
        // Terminal-specific components
        TerminalComponent,        // Marks this as a terminal
        texture.dimensions(),     // Terminal dimensions for input
        TerminalInput::default(), // Enable keyboard and mouse input
        // Your custom marker component
        MainTerminal,
        // You could add any other components here!
        // Examples: custom markers, physics, animations, visibility, etc.
    ));

    // Step 8: Store the texture and material in a resource
    commands.insert_resource(MyTerminal {
        texture,
        material_handle: material,
    });
    commands.insert_resource(AppState::default());

    info!("Manual 3D setup complete!");
    info!("You now have full control over the 3D terminal entity");
}

fn handle_input(
    mut events: MessageReader<TerminalEvent>,
    mut state: ResMut<AppState>,
    query: Query<Entity, With<MainTerminal>>,
) {
    let terminal_entity = match query.single() {
        Ok(entity) => entity,
        Err(_) => return,
    };

    for event in events.read().filter(|e| e.target == terminal_entity) {
        match &event.event {
            TerminalEventType::KeyPress { key, .. } => {
                let key_str = format!("{:?}", key);
                info!("Key: {}", key_str);
                state.last_key = Some(key_str);
                state.counter += 1;
            }
            TerminalEventType::MouseMove { position } => {
                state.mouse_pos = Some(*position);
            }
            TerminalEventType::MousePress { position, button } => {
                info!(
                    "3D Click at ({}, {}) with {:?}",
                    position.0, position.1, button
                );
                state.counter += 1;
            }
            _ => {}
        }
    }
}

fn rotate_terminal(time: Res<Time>, mut query: Query<&mut Transform, With<MainTerminal>>) {
    for mut transform in &mut query {
        // Seesaw rotation: oscillate ±45° around Y axis
        let angle = (time.elapsed_secs() * 0.8).sin() * std::f32::consts::FRAC_PI_4;
        transform.rotation =
            Quat::from_rotation_x(std::f32::consts::FRAC_PI_2) * Quat::from_rotation_z(angle);
    }
}

fn render_terminal(
    mut terminal_res: ResMut<MyTerminal>,
    state: Res<AppState>,
    render_device: Res<RenderDevice>,
    render_queue: Res<RenderQueue>,
    mut images: ResMut<Assets<Image>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    time: Res<Time>,
) {
    let counter = state.counter;
    let last_key = state.last_key.as_deref().unwrap_or("None");
    let mouse_info = state
        .mouse_pos
        .map(|(col, row)| format!("col={}, row={}", col, row))
        .unwrap_or_else(|| "Not moved yet".to_string());
    let rotation_angle = (time.elapsed_secs() * 0.8).sin() * 45.0;

    // Step 9: Update the terminal texture
    terminal_res
        .texture
        .update(&render_device, &render_queue, &mut images, |frame| {
            let area = frame.area();

            let chunks = Layout::default()
                .direction(Direction::Vertical)
                .constraints([
                    Constraint::Length(3),
                    Constraint::Min(0),
                    Constraint::Length(3),
                ])
                .split(area);

            // Title
            let title = Paragraph::new(format!(
                "TerminalTexture Manual Setup - 3D | Seesaw: {:.1}°",
                rotation_angle
            ))
            .style(Style::default().fg(RatatuiColor::Cyan).bold())
            .alignment(Alignment::Center)
            .block(Block::bordered());
            frame.render_widget(title, chunks[0]);

            // Content
            let content = vec![
                Line::from(""),
                Line::from("Manual 3D Terminal Setup with TerminalTexture")
                    .style(Style::default().fg(RatatuiColor::Yellow)),
                Line::from(""),
                Line::from("This example shows manual 3D setup:"),
                Line::from("  1. TerminalTexture::create() - Creates texture + terminal"),
                Line::from("  2. Manual mesh creation - Custom shape and size"),
                Line::from("  3. Manual material setup - Custom properties"),
                Line::from("  4. Manual entity spawning - Full 3D control"),
                Line::from("  5. TerminalTexture::update() - Render your UI"),
                Line::from("  6. Manual material update - Trigger change detection"),
                Line::from(""),
                Line::from("3D Benefits:").style(Style::default().fg(RatatuiColor::Green)),
                Line::from("  • Custom mesh shapes (not just planes)"),
                Line::from("  • Custom material properties"),
                Line::from("  • Full transform control (position, rotation, scale)"),
                Line::from("  • 3D raycasting for mouse input"),
                Line::from(""),
                Line::from(
                    "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━",
                )
                .style(Style::default().fg(RatatuiColor::DarkGray)),
                Line::from(""),
                Line::from(vec![
                    Span::styled("Counter: ", Style::default().fg(RatatuiColor::Yellow)),
                    Span::styled(
                        format!("{}", counter),
                        Style::default().fg(RatatuiColor::Cyan).bold(),
                    ),
                ]),
                Line::from(vec![
                    Span::styled("Last Key: ", Style::default().fg(RatatuiColor::Yellow)),
                    Span::styled(last_key, Style::default().fg(RatatuiColor::Cyan)),
                ]),
                Line::from(vec![
                    Span::styled("Mouse: ", Style::default().fg(RatatuiColor::Yellow)),
                    Span::raw(mouse_info),
                ]),
                Line::from(vec![
                    Span::styled("Uptime: ", Style::default().fg(RatatuiColor::Yellow)),
                    Span::raw(format!("{:.1}s", time.elapsed_secs())),
                ]),
            ];

            let para =
                Paragraph::new(content).block(Block::bordered().title("Manual 3D Setup Demo"));
            frame.render_widget(para, chunks[1]);

            // Status bar
            let status =
                Paragraph::new("Seesaw motion in 3D | Click to interact | Manual 3D control")
                    .style(Style::default().fg(RatatuiColor::DarkGray));
            frame.render_widget(status, chunks[2]);
        });

    // Step 10: Manually update the material to trigger Bevy's change detection
    // This is required for 3D materials (not needed for 2D ImageNode)
    if let Some(material) = materials.get_mut(&terminal_res.material_handle) {
        material.base_color_texture = Some(terminal_res.texture.image_handle());
    }
}
