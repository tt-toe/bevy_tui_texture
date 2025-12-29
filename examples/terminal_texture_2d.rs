// TerminalTexture 2D Example
//
// Demonstrates manual terminal setup using only TerminalTexture API.
// This gives you full control over entity creation and management.
//
// Use this when you need:
// - Custom entity setup with specific components
// - Fine-grained control over positioning and styling
// - Integration with existing entity hierarchies
// - Multiple terminals with different configurations
//
// For simpler use cases, use SimpleTerminal2D instead.

use std::sync::Arc;

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
                title: "TerminalTexture 2D - Manual Setup Example".to_string(),
                resolution: WindowResolution::new(1024, 768),
                ..default()
            }),
            ..default()
        }))
        .add_plugins(TerminalPlugin::default())
        .add_systems(Startup, setup)
        .add_systems(Update, handle_input.in_set(TerminalSystemSet::UserUpdate))
        .add_systems(Update, render_terminal.in_set(TerminalSystemSet::Render))
        .run();
}

// Resource holding our manually created terminal texture
#[derive(Resource)]
struct MyTerminal {
    texture: TerminalTexture,
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
    render_device: Res<RenderDevice>,
    render_queue: Res<RenderQueue>,
    mut images: ResMut<Assets<Image>>,
) {
    info!("=== Manual 2D Terminal Setup with TerminalTexture ===");

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

    // Step 3: Manually spawn the camera
    commands.spawn(Camera2d);

    // Step 4: Manually spawn the terminal entity with custom components
    // This is where you have full control!
    commands.spawn((
        // Standard Bevy UI components
        ImageNode {
            image: texture.image_handle(),
            ..default()
        },
        Node {
            width: Val::Px(texture.width as f32),
            height: Val::Px(texture.height as f32),
            left: Val::Px(10.0), // Custom positioning
            top: Val::Px(10.0),
            ..default()
        },
        GlobalTransform::default(),
        // Terminal-specific components
        TerminalComponent,        // Marks this as a terminal
        texture.dimensions(),     // Terminal dimensions for input
        TerminalInput::default(), // Enable keyboard and mouse input
        // Your custom marker component
        MainTerminal,
        // You could add any other components here!
        // Examples: custom markers, transform animations, visibility, etc.
    ));

    // Step 5: Store the texture in a resource
    commands.insert_resource(MyTerminal { texture });
    commands.insert_resource(AppState::default());

    info!("Manual setup complete!");
    info!("You now have full control over the terminal entity");
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
                    "Click at ({}, {}) with {:?}",
                    position.0, position.1, button
                );
                state.counter += 1;
            }
            _ => {}
        }
    }
}

fn render_terminal(
    mut terminal_res: ResMut<MyTerminal>,
    state: Res<AppState>,
    render_device: Res<RenderDevice>,
    render_queue: Res<RenderQueue>,
    mut images: ResMut<Assets<Image>>,
    time: Res<Time>,
) {
    let elapsed = time.elapsed_secs();
    let counter = state.counter;
    let last_key = state.last_key.as_deref().unwrap_or("None");
    let mouse_info = state
        .mouse_pos
        .map(|(col, row)| format!("col={}, row={}", col, row))
        .unwrap_or_else(|| "Not moved yet".to_string());

    // Step 6: Update the terminal texture
    // This is where you render your UI
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
            let title = Paragraph::new("TerminalTexture Manual Setup - 2D")
                .style(Style::default().fg(RatatuiColor::Cyan).bold())
                .alignment(Alignment::Center)
                .block(Block::bordered());
            frame.render_widget(title, chunks[0]);

            // Content
            let content = vec![
                Line::from(""),
                Line::from("Manual Terminal Setup with TerminalTexture")
                    .style(Style::default().fg(RatatuiColor::Yellow)),
                Line::from(""),
                Line::from("This example shows how to use TerminalTexture directly:"),
                Line::from("  1. TerminalTexture::create() - Creates texture + terminal"),
                Line::from("  2. Manual entity spawning - Full control over components"),
                Line::from("  3. TerminalTexture::update() - Render your UI"),
                Line::from(""),
                Line::from("Benefits:").style(Style::default().fg(RatatuiColor::Green)),
                Line::from("  • Full control over entity setup"),
                Line::from("  • Add custom components and markers"),
                Line::from("  • Custom positioning and styling"),
                Line::from("  • Integration with existing hierarchies"),
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
                    Span::raw(format!("{:.1}s", elapsed)),
                ]),
            ];

            let para = Paragraph::new(content).block(Block::bordered().title("Manual Setup Demo"));
            frame.render_widget(para, chunks[1]);

            // Status bar
            let status = Paragraph::new("Press keys or click to interact | Manual entity control")
                .style(Style::default().fg(RatatuiColor::DarkGray));
            frame.render_widget(status, chunks[2]);
        });
}
