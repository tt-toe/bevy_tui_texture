//! # Multiple Terminals - Advanced Layout Example
//!
//! **Demonstrates managing multiple independent terminals** with different roles and interactions.
//!
//! ## What This Example Shows
//!
//! - **Multiple Terminal Setup** - Creating and managing 5 separate terminals
//! - **Tiled Layout** - Positioning terminals in a grid with gaps
//! - **Z-Ordering** - Overlapping terminals with proper layering
//! - **Isolated State** - Each terminal maintains independent state
//! - **Event Routing** - Entity-targeted events to specific terminals
//! - **Mixed Interaction Modes** - Interactive vs display-only terminals
//!
//! ## Running
//!
//! ```bash
//! cargo run --example multiple_terminals
//! ```
//!
//! ## Terminal Layout
//!
//! ```
//! ┌─────────────┬─────────────┐
//! │ Interactive │     Log     │
//! │   Terminal  │   Terminal  │
//! ├─────────────┼─────────────┤
//! │   Status    │  Overlap 1  │
//! │   Terminal  │  Overlap 2  │ (Layered)
//! └─────────────┴─────────────┘
//! ```
//!
//! ## Controls
//!
//! - **Click Terminals** - Focus and interact with different terminals
//! - **Click List Items** - Select items in interactive terminal
//! - **Click Log** - Add log entries
//! - **Click Status** - Increment counter
//! - **ESC** - Quit application
//!
//! ## Architecture Highlights
//!
//! - Uses `TerminalBundle::ui()` for each terminal - each terminal is a
//!   `Tui` Component on its own entity, no wrapping Resource
//! - Demonstrates entity-based terminal identification via marker components
//! - Shows how to route `TerminalEvent` to specific terminals
//! - Each terminal's draw system takes zero render-resource parameters

use bevy::prelude::*;
use bevy::render::renderer::{RenderDevice, RenderQueue};
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
                title: "Multiple Terminals".to_string(),
                resolution: WindowResolution::new(1024, 768),
                ..default()
            }),
            ..default()
        }))
        .add_plugins(TerminalPlugin::default())
        .add_systems(Startup, setup_terminals)
        .add_systems(Update, handle_events.in_set(TerminalSystemSet::UserUpdate))
        .add_systems(
            Update,
            (
                update_interactive,
                update_log,
                update_status,
                update_overlap_back,
                update_overlap_front,
            )
                .in_set(TerminalSystemSet::Render),
        )
        .run();
}

// App state for interactive terminal
#[derive(Resource, Default)]
struct AppState {
    counter: usize,
    log_messages: Vec<String>,
    selected_item: usize,
}

// Marker components for each terminal type
#[derive(Component)]
struct InteractiveTerminal;

#[derive(Component)]
struct LogTerminal;

#[derive(Component)]
struct StatusTerminal;

#[derive(Component)]
struct OverlapBackTerminal;

#[derive(Component)]
struct OverlapFrontTerminal;

fn setup_terminals(
    mut commands: Commands,
    render_device: Res<RenderDevice>,
    render_queue: Res<RenderQueue>,
    mut images: ResMut<Assets<Image>>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    info!("Setting up multiple terminals with easy setup API...");

    // Load font (shared across all terminals)
    let font_data = include_bytes!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/assets/fonts/fusion-pixel-10px-monospaced-ja.ttf"
    ));
    let font = TerminalFont::new(font_data).expect("Failed to load font");
    let fonts = Arc::new(Fonts::new(font, 16));

    // Get font metrics for positioning
    let char_width_px = fonts.min_width_px();
    let char_height_px = fonts.height_px();

    // Define gap size between terminals
    const GAP: f32 = 20.0;

    // Define col/row num between terminals
    const INTERACTIVE_COL: u16 = 40;
    const INTERACTIVE_ROW: u16 = 27;
    const LOG_COL: u16 = 50;
    const LOG_ROW: u16 = 10;
    const STATUS_COL: u16 = 80;
    const STATUS_ROW: u16 = 7;

    // Calculate positions for tile layout
    let interactive_width = INTERACTIVE_COL * char_width_px as u16;
    let interactive_height = INTERACTIVE_ROW * char_height_px as u16;
    let log_height = LOG_ROW * char_height_px as u16;

    let interactive_pos = (GAP, GAP);
    let log_pos = (GAP + interactive_width as f32 + GAP, GAP);
    let status_pos = (GAP, GAP + interactive_height as f32 + GAP);
    let overlap_back_pos = (log_pos.0, log_pos.1 + log_height as f32 + GAP);
    let overlap_front_pos = (overlap_back_pos.0 + 50.0, overlap_back_pos.1 + 50.0);

    // Create camera FIRST
    commands.spawn(Camera2d);

    let mut ctx = TerminalSpawnCtx {
        render_device: &render_device,
        render_queue: &render_queue,
        images: &mut images,
        meshes: &mut meshes,
        materials: &mut materials,
    };

    // Create interactive terminal (top-left) with full input
    let interactive_bundle = TerminalBundle::ui(
        INTERACTIVE_COL,
        INTERACTIVE_ROW,
        fonts.clone(),
        TerminalConfig {
            programmatic_glyphs: true,
            keyboard: true,
            mouse: true,
            ..default()
        },
        &mut ctx,
    )
    .expect("Failed to create interactive terminal");
    commands.spawn((
        interactive_bundle,
        Node {
            position_type: PositionType::Absolute,
            left: Val::Px(interactive_pos.0),
            top: Val::Px(interactive_pos.1),
            ..default()
        },
        InteractiveTerminal,
    ));

    // Create log terminal (top-right) with mouse input only
    let log_bundle = TerminalBundle::ui(
        LOG_COL,
        LOG_ROW,
        fonts.clone(),
        TerminalConfig {
            programmatic_glyphs: false,
            keyboard: false,
            mouse: true,
            ..default()
        },
        &mut ctx,
    )
    .expect("Failed to create log terminal");
    commands.spawn((
        log_bundle,
        Node {
            position_type: PositionType::Absolute,
            left: Val::Px(log_pos.0),
            top: Val::Px(log_pos.1),
            ..default()
        },
        LogTerminal,
    ));

    // Create status terminal (bottom-left) with mouse input
    let status_bundle = TerminalBundle::ui(
        STATUS_COL,
        STATUS_ROW,
        fonts.clone(),
        TerminalConfig {
            programmatic_glyphs: false,
            keyboard: false,
            mouse: true,
            ..default()
        },
        &mut ctx,
    )
    .expect("Failed to create status terminal");
    commands.spawn((
        status_bundle,
        Node {
            position_type: PositionType::Absolute,
            left: Val::Px(status_pos.0),
            top: Val::Px(status_pos.1),
            ..default()
        },
        StatusTerminal,
    ));

    // Create overlapping back terminal (lower z-index)
    let overlap_back_bundle = TerminalBundle::ui(
        40,
        12,
        fonts.clone(),
        TerminalConfig {
            programmatic_glyphs: false,
            keyboard: false,
            mouse: true,
            ..default()
        },
        &mut ctx,
    )
    .expect("Failed to create overlap back terminal");
    commands.spawn((
        overlap_back_bundle,
        Node {
            position_type: PositionType::Absolute,
            left: Val::Px(overlap_back_pos.0),
            top: Val::Px(overlap_back_pos.1),
            ..default()
        },
        ZIndex(0),
        OverlapBackTerminal,
    ));

    // Create overlapping front terminal (higher z-index)
    let overlap_front_bundle = TerminalBundle::ui(
        40,
        12,
        fonts.clone(),
        TerminalConfig {
            programmatic_glyphs: false,
            keyboard: false,
            mouse: true,
            ..default()
        },
        &mut ctx,
    )
    .expect("Failed to create overlap front terminal");
    commands.spawn((
        overlap_front_bundle,
        Node {
            position_type: PositionType::Absolute,
            left: Val::Px(overlap_front_pos.0),
            top: Val::Px(overlap_front_pos.1),
            ..default()
        },
        ZIndex(10),
        OverlapFrontTerminal,
    ));

    // Initialize app state
    let mut app_state = AppState::default();
    app_state
        .log_messages
        .push("Application started".to_string());
    app_state
        .log_messages
        .push("Five terminals initialized with easy setup API".to_string());
    commands.insert_resource(app_state);

    info!("Multiple terminals setup complete!");
}

fn handle_events(
    mut events: MessageReader<TerminalEvent>,
    mut app_state: ResMut<AppState>,
    interactive_query: Query<Entity, With<InteractiveTerminal>>,
    log_query: Query<Entity, With<LogTerminal>>,
    status_query: Query<Entity, With<StatusTerminal>>,
    overlap_back_query: Query<Entity, With<OverlapBackTerminal>>,
    overlap_front_query: Query<Entity, With<OverlapFrontTerminal>>,
) {
    let interactive_entity = interactive_query.single().ok();
    let log_entity = log_query.single().ok();
    let status_entity = status_query.single().ok();
    let overlap_back_entity = overlap_back_query.single().ok();
    let overlap_front_entity = overlap_front_query.single().ok();

    for event in events.read() {
        // Handle interactive terminal events
        if Some(event.target) == interactive_entity {
            match &event.event {
                TerminalEventType::MousePress { position, .. } => {
                    let (col, row) = *position;
                    info!("[Interactive] Click at col={}, row={}", col, row);

                    app_state.log_messages.push(format!(
                        "[Interactive] Mouse clicked at col={}, row={}",
                        col, row
                    ));

                    // Check if clicking on list items (list is at rows 4-14 approximately)
                    if (4..=14).contains(&row) {
                        let item_index = (row - 4) as usize;
                        if item_index < 5 {
                            app_state.selected_item = item_index;
                            app_state
                                .log_messages
                                .push(format!("Selected item {}", item_index));
                        }
                    }
                    app_state.counter += 1;
                }
                TerminalEventType::KeyPress { key, .. } => match key {
                    KeyCode::ArrowUp => {
                        app_state.selected_item = app_state.selected_item.saturating_sub(1);
                        app_state
                            .log_messages
                            .push("Selection moved up".to_string());
                    }
                    KeyCode::ArrowDown => {
                        app_state.selected_item = (app_state.selected_item + 1).min(4);
                        app_state
                            .log_messages
                            .push("Selection moved down".to_string());
                    }
                    KeyCode::Enter => {
                        let item = app_state.selected_item;
                        app_state
                            .log_messages
                            .push(format!("Selected item {}", item));
                        app_state.counter += 1;
                    }
                    KeyCode::KeyC => {
                        app_state.counter += 1;
                        let count = app_state.counter;
                        app_state.log_messages.push(format!("Counter: {}", count));
                    }
                    _ => {}
                },
                _ => {}
            }
        }

        // Handle log terminal events
        if Some(event.target) == log_entity
            && let TerminalEventType::MousePress { position, .. } = &event.event {
                let (col, row) = *position;
                info!("[Log] Click at col={}, row={}", col, row);

                app_state.log_messages.push(format!(
                    "[Log] Clicked at col={}, row={} - This is display-only!",
                    col, row
                ));
            }

        // Handle status terminal events (clicking gauge to adjust)
        if Some(event.target) == status_entity
            && let TerminalEventType::MousePress { position, .. } = &event.event {
                let (col, _row) = *position;
                info!("[Status] Click at col={}", col);

                app_state.log_messages.push(format!(
                    "[Status] Clicked at col={} - Adjusting gauge!",
                    col
                ));
                // Simple gauge interaction - click anywhere to increment counter
                app_state.counter = (app_state.counter + 10).min(100);
            }

        // Handle overlap back terminal events
        if Some(event.target) == overlap_back_entity
            && let TerminalEventType::MousePress { position, .. } = &event.event {
                let (col, row) = *position;
                info!("[Overlap BACK] col={}, row={} | ZIndex=0", col, row);

                app_state
                    .log_messages
                    .push(format!("[BACK Z=0] col={}, row={}", col, row));
            }

        // Handle overlap front terminal events
        if Some(event.target) == overlap_front_entity
            && let TerminalEventType::MousePress { position, .. } = &event.event {
                let (col, row) = *position;
                info!("[Overlap FRONT] col={}, row={} | ZIndex=10", col, row);

                app_state
                    .log_messages
                    .push(format!("[FRONT Z=10] col={}, row={}", col, row));
            }

        // Keep log size manageable
        if app_state.log_messages.len() > 8 {
            app_state.log_messages.remove(0);
        }
    }
}

/// Zero render-resource parameters: `gpu_flush_system` (registered by
/// `TerminalPlugin`) owns the GPU render + async copy for every
/// `Tui`-carrying entity, so each of these five systems only needs its own
/// marker-filtered query plus whatever gameplay state it renders.
fn update_interactive(
    mut screens: Query<&mut Tui, With<InteractiveTerminal>>,
    app_state: Res<AppState>,
) {
    let Ok(mut term) = screens.single_mut() else {
        return;
    };
    let selected_item = app_state.selected_item;
    term.draw(|frame| {
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
        let title = Paragraph::new("Interactive Terminal")
            .style(Style::default().fg(RatatuiColor::Yellow).bold())
            .block(Block::bordered());
        frame.render_widget(title, chunks[0]);

        // Selectable list
        let items: Vec<ListItem> = (0..5)
            .map(|i| ListItem::new(format!("Item {} - Click or use arrows", i)))
            .collect();

        let mut list_state = ListState::default().with_selected(Some(selected_item));
        let list = List::new(items)
            .block(Block::bordered().title("Select an item"))
            .highlight_style(
                Style::default()
                    .bg(RatatuiColor::Cyan)
                    .fg(RatatuiColor::Black)
                    .add_modifier(Modifier::BOLD),
            )
            .highlight_symbol(">> ");

        frame.render_stateful_widget(list, chunks[1], &mut list_state);

        // Instructions
        let help = Paragraph::new(vec![Line::from("↑/↓: Move  Enter: Select  C: Counter")])
            .style(Style::default().fg(RatatuiColor::DarkGray))
            .block(Block::bordered());
        frame.render_widget(help, chunks[2]);
    });
}

fn update_log(mut screens: Query<&mut Tui, With<LogTerminal>>, app_state: Res<AppState>) {
    let Ok(mut term) = screens.single_mut() else {
        return;
    };
    let log_messages = &app_state.log_messages;
    term.draw(|frame| {
        let area = frame.area();

        let recent_logs: Vec<Line> = log_messages
            .iter()
            .rev()
            .take(12)
            .rev()
            .enumerate()
            .map(|(i, msg)| {
                let style = if i == log_messages.len().saturating_sub(1) {
                    Style::default().fg(RatatuiColor::Green)
                } else {
                    Style::default().fg(RatatuiColor::Gray)
                };
                Line::from(format!("• {}", msg)).style(style)
            })
            .collect();

        let logs = Paragraph::new(recent_logs)
            .block(Block::bordered().title("Log Terminal (Click to test!)"))
            .wrap(Wrap { trim: true });

        frame.render_widget(logs, area);
    });
}

fn update_status(
    mut screens: Query<&mut Tui, With<StatusTerminal>>,
    app_state: Res<AppState>,
    time: Res<Time>,
) {
    let Ok(mut term) = screens.single_mut() else {
        return;
    };
    let counter = app_state.counter;
    let elapsed = time.elapsed_secs();
    let fps = 1.0 / time.delta_secs();
    term.draw(|frame| {
        let area = frame.area();

        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Length(3), Constraint::Min(0)])
            .split(area);

        let title = Paragraph::new("Status Terminal")
            .style(Style::default().fg(RatatuiColor::Magenta).bold())
            .block(Block::bordered());
        frame.render_widget(title, chunks[0]);

        let gauge_value = ((elapsed.sin() + 1.0) * 50.0) as u16;
        let status_line = format!(
            "Counter: {}  |  FPS: {:.1}  |  Uptime: {:.1}s",
            counter, fps, elapsed
        );

        let gauge = Gauge::default()
            .block(Block::bordered().title("Activity (Click to add +10!)"))
            .gauge_style(Style::default().fg(RatatuiColor::Cyan))
            .percent(gauge_value.min(100));

        let status = Paragraph::new(status_line);

        let inner_chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Length(1), Constraint::Length(3)])
            .split(chunks[1]);

        frame.render_widget(status, inner_chunks[0]);
        frame.render_widget(gauge, inner_chunks[1]);
    });
}

fn update_overlap_back(mut screens: Query<&mut Tui, With<OverlapBackTerminal>>) {
    let Ok(mut term) = screens.single_mut() else {
        return;
    };
    term.draw(|frame| {
        let area = frame.area();

        let text = vec![
            Line::from(""),
            Line::from("Overlap BACK Terminal")
                .style(Style::default().fg(RatatuiColor::Red).bold()),
            Line::from("ZIndex = 0"),
            Line::from(""),
            Line::from("This terminal is BEHIND")
                .style(Style::default().fg(RatatuiColor::Yellow)),
            Line::from("the front terminal."),
            Line::from(""),
            Line::from("Click in the overlap area"),
            Line::from("to test z-ordering!"),
        ];

        let para = Paragraph::new(text)
            .block(Block::bordered().title("BACK"))
            .alignment(Alignment::Center);

        frame.render_widget(para, area);
    });
}

fn update_overlap_front(
    mut screens: Query<&mut Tui, With<OverlapFrontTerminal>>,
    time: Res<Time>,
) {
    let Ok(mut term) = screens.single_mut() else {
        return;
    };
    let elapsed = time.elapsed_secs();
    term.draw(|frame| {
        let area = frame.area();

        let pulse = ((elapsed * 2.0).sin() + 1.0) / 2.0;
        let color = if pulse > 0.5 {
            RatatuiColor::Green
        } else {
            RatatuiColor::Cyan
        };

        let text = vec![
            Line::from(""),
            Line::from("Overlap FRONT Terminal").style(Style::default().fg(color).bold()),
            Line::from("ZIndex = 10"),
            Line::from(""),
            Line::from("This terminal is ON TOP")
                .style(Style::default().fg(RatatuiColor::Yellow)),
            Line::from("of the back terminal."),
            Line::from(""),
            Line::from("Clicks here should show")
                .style(Style::default().fg(RatatuiColor::Magenta)),
            Line::from("[FRONT] in the log!").style(Style::default().fg(RatatuiColor::Magenta)),
        ];

        let para = Paragraph::new(text)
            .block(Block::bordered().title("FRONT"))
            .alignment(Alignment::Center);

        frame.render_widget(para, area);
    });
}
