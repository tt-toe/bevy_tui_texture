//! WASM Browser Demo - Widget Catalog 3D in Browser
//!
//! This example demonstrates running Bevy + ratatui widget catalog in the browser with WebAssembly.
//!
//! ## Build & Run
//!
//! ```bash
//! # Build WASM
//! cargo wasm
//!
//! # Serve with custom web server
//! cargo run --example web_server
//! # Then open http://127.0.0.1:8080
//! ```

use std::sync::Arc;
use std::time::Duration;

use bevy::pbr::StandardMaterial;
use bevy::prelude::*;
use bevy::render::renderer::{RenderDevice, RenderQueue};
use ratatui::prelude::*;
use ratatui::style::Color as RatatuiColor;
use ratatui::widgets::*;
use unicode_width::UnicodeWidthStr;

use bevy_tui_texture::Font as TerminalFont;
use bevy_tui_texture::prelude::*;

#[cfg(target_arch = "wasm32")]
use wasm_bindgen::prelude::*;

#[cfg(target_arch = "wasm32")]
macro_rules! console_log {
    ($($t:tt)*) => (web_sys::console::log_1(&format!($($t)*).into()))
}

#[cfg(not(target_arch = "wasm32"))]
macro_rules! console_log {
    ($($t:tt)*) => (println!($($t)*))
}

// Terminal dimensions
const COLS: u16 = 100;
const ROWS: u16 = 30;

// WASM entry point
#[cfg(target_arch = "wasm32")]
#[wasm_bindgen(start)]
pub fn wasm_main() {
    console_error_panic_hook::set_once();
    console_log!("WASM main started, calling main()...");
    main();
}

fn main() {
    console_log!("main() called");
    let mut app = App::new();
    console_log!("App created");

    app.add_plugins(DefaultPlugins.set(WindowPlugin {
        primary_window: Some(Window {
            title: "bevy_tui_texture - Widget Catalog 3D (WASM)".to_string(),
            canvas: Some("#bevy".to_string()),
            fit_canvas_to_parent: true,
            prevent_default_event_handling: false,
            ..default()
        }),
        ..default()
    }))
    .add_plugins(TerminalPlugin::default());
    console_log!("Plugins added");

    app.add_systems(Startup, setup_terminal)
        .add_systems(
            Update,
            handle_terminal_events.in_set(TerminalSystemSet::UserUpdate),
        )
        .add_systems(Update, (update_terminal_content, rotate_plane));
    console_log!("Systems added, calling app.run()...");

    app.run();
    console_log!("app.run() returned (should not see this in WASM)");
}

/// UI state for tracking interactions
#[derive(Resource)]
struct WidgetCatalogState {
    terminal: SimpleTerminal3D,

    selected_tab: usize,
    list_state: ListState,
    selected_button: Option<usize>,
    gauge_value: u16,
    sparkline_data: Vec<u64>,
    sparkline_timer: Timer,
    counter: usize,
    mouse_position: Option<(u16, u16)>,

    // Store layout rectangles for accurate hit testing
    button_rects: Vec<ratatui::layout::Rect>,
    h_button_rects: Vec<ratatui::layout::Rect>,
    list_inner_rect: Option<ratatui::layout::Rect>,
    gauge_inner_rect: Option<ratatui::layout::Rect>,
}

/// Marker component for the rotating plane
#[derive(Component)]
struct RotatingPlane;

fn setup_terminal(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    render_device: Res<RenderDevice>,
    render_queue: Res<RenderQueue>,
    mut images: ResMut<Assets<Image>>,
) {
    console_log!("Setting up 3D widget catalog terminal...");

    // Load font
    let font_data = include_bytes!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/assets/fonts/Mplus1Code-Regular.ttf"
    ));
    console_log!("Font data loaded: {} bytes", font_data.len());

    let font = TerminalFont::new(font_data).expect("Failed to load font");
    let fonts = Arc::new(Fonts::new(font, 16));
    console_log!("Fonts created");

    // Spawn 3D camera
    commands.spawn((
        Camera3d::default(),
        Transform::from_xyz(0.0, 0.0, 800.0).looking_at(Vec3::ZERO, Vec3::Y),
    ));
    console_log!("Camera spawned");

    // Add ambient light
    commands.insert_resource(AmbientLight {
        color: bevy::color::Color::WHITE,
        brightness: 1.0,
        affects_lightmapped_meshes: true,
    });
    console_log!("Ambient light added");

    // Create 3D terminal with easy setup API
    console_log!("Creating 3D terminal...");
    let terminal = SimpleTerminal3D::create_and_spawn(
        COLS,
        ROWS,
        fonts,
        Vec3::ZERO,                                          // Position at origin
        Quat::from_rotation_x(-std::f32::consts::FRAC_PI_2), // Face camera
        Vec3::ONE,                                           // Normal scale
        RotatingPlane,                                       // Marker component
        true,                                                // Enable programmatic glyphs
        true,                                                // Enable keyboard
        true,                                                // Enable mouse
        &mut commands,
        &mut meshes,
        &mut materials,
        &render_device,
        &render_queue,
        &mut images,
    )
    .expect("Failed to create 3D terminal");
    console_log!("3D terminal created successfully!");

    let terminal_entity = terminal.entity();

    // Create state with terminal and initial values
    commands.insert_resource(WidgetCatalogState {
        terminal,
        selected_tab: 0,
        list_state: ListState::default().with_selected(Some(0)),
        selected_button: None,
        gauge_value: 60,
        sparkline_data: vec![2, 5, 3, 8, 6, 9, 4, 7, 5, 8, 6, 10, 8, 6, 9, 11],
        sparkline_timer: Timer::new(Duration::from_millis(100), TimerMode::Repeating),
        counter: 0,
        mouse_position: None,
        button_rects: Vec::new(),
        h_button_rects: Vec::new(),
        list_inner_rect: None,
        gauge_inner_rect: None,
    });

    console_log!(
        "3D widget catalog setup complete! Terminal entity: {:?}",
        terminal_entity
    );
}

/// Handle terminal input events (mouse clicks, hover, etc.)
fn handle_terminal_events(
    mut events: MessageReader<TerminalEvent>,
    mut ui_state: ResMut<WidgetCatalogState>,
    query: Query<Entity, With<RotatingPlane>>,
) {
    let terminal_entity = match query.single() {
        Ok(entity) => entity,
        Err(_) => return,
    };

    for event in events.read().filter(|e| e.target == terminal_entity) {
        match &event.event {
            TerminalEventType::MouseMove { position } => {
                ui_state.mouse_position = Some(*position);
            }

            TerminalEventType::MousePress { position, .. } => {
                let (col, row) = *position;
                let pos = ratatui::layout::Position { x: col, y: row };

                console_log!(
                    "3D Mouse Press: col={}, row={}, target={:?}",
                    col,
                    row,
                    event.target
                );

                // Tab detection (manual calculation as tabs are not stored)
                let chunks = Layout::default()
                    .direction(Direction::Vertical)
                    .constraints([
                        Constraint::Length(1),
                        Constraint::Length(3),
                        Constraint::Min(0),
                    ])
                    .split(ratatui::layout::Rect {
                        x: 0,
                        y: 0,
                        width: COLS,
                        height: ROWS,
                    });

                if row >= chunks[1].y && row < chunks[1].y + chunks[1].height {
                    let tab_labels = ["Buttons", "Lists", "Charts", "Interactive", "Glyphs"];
                    let mut col_pos = 2;

                    for (i, label) in tab_labels.iter().enumerate() {
                        let label_width = label.width();
                        let start = col_pos;
                        let end = col_pos + label_width - 1;

                        if col >= start as u16 && col <= end as u16 {
                            ui_state.selected_tab = i;
                            break;
                        }

                        col_pos = col_pos + label_width + 3;
                    }
                }

                // Buttons tab (0)
                if ui_state.selected_tab == 0 {
                    // Vertical buttons
                    for (i, rect) in ui_state.button_rects.iter().enumerate() {
                        if rect.contains(pos) {
                            ui_state.selected_button = Some(i);
                            match i {
                                0 => ui_state.counter += 1,
                                1 => ui_state.gauge_value = (ui_state.gauge_value + 10).min(100),
                                2 => ui_state.gauge_value = ui_state.gauge_value.saturating_sub(10),
                                _ => {}
                            }
                            break;
                        }
                    }

                    // Horizontal buttons
                    for (i, rect) in ui_state.h_button_rects.iter().enumerate() {
                        if rect.contains(pos) {
                            ui_state.selected_button = Some(i + 3);
                            ui_state.counter += 1;
                            break;
                        }
                    }
                }

                // Lists tab (1)
                if ui_state.selected_tab == 1
                    && let Some(inner) = ui_state.list_inner_rect
                    && inner.contains(pos)
                {
                    let index = (row - inner.y) as usize;
                    ui_state.list_state.select(Some(index.min(9)));
                }

                // Interactive tab (3)
                if ui_state.selected_tab == 3
                    && let Some(inner) = ui_state.gauge_inner_rect
                    && inner.contains(pos)
                {
                    let percentage =
                        ((col - inner.x) as f32 / inner.width as f32 * 100.0) as u16;
                    ui_state.gauge_value = percentage.min(100);
                }
            }

            TerminalEventType::KeyPress { key, .. } => {
                use KeyCode::*;
                match key {
                    Tab => {
                        ui_state.selected_tab = (ui_state.selected_tab + 1) % 5;
                    }
                    ArrowUp => {
                        if ui_state.selected_tab == 1 {
                            let i = ui_state.list_state.selected().unwrap_or(0);
                            ui_state.list_state.select(Some(i.saturating_sub(1)));
                        }
                    }
                    ArrowDown => {
                        if ui_state.selected_tab == 1 {
                            let i = ui_state.list_state.selected().unwrap_or(0);
                            ui_state.list_state.select(Some((i + 1).min(9)));
                        }
                    }
                    ArrowLeft => {
                        ui_state.gauge_value = ui_state.gauge_value.saturating_sub(5);
                    }
                    ArrowRight => {
                        ui_state.gauge_value = (ui_state.gauge_value + 5).min(100);
                    }
                    _ => {}
                }
            }

            _ => {}
        }
    }
}

/// Update terminal content and render to 3D mesh
fn update_terminal_content(
    mut ui_state: ResMut<WidgetCatalogState>,
    render_device: Res<RenderDevice>,
    render_queue: Res<RenderQueue>,
    mut images: ResMut<Assets<Image>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    marker_query: Query<&MeshMaterial3d<StandardMaterial>, With<RotatingPlane>>,
    time: Res<Time>,
) {
    // Update sparkline data with time-based pseudo-random values
    ui_state.sparkline_timer.tick(time.delta());
    if ui_state.sparkline_timer.just_finished() {
        // Use multiple sine waves at different frequencies for pseudo-random effect
        let t = time.elapsed_secs();
        let new_value = ((t * 3.7).sin() * 4.0
            + (t * 7.3).sin() * 3.0
            + (t * 11.1).sin() * 2.0
            + 10.0) as u64;
        ui_state.sparkline_data.push(new_value.clamp(1, 15));
        if ui_state.sparkline_data.len() > 32 {
            ui_state.sparkline_data.remove(0);
        }
    }

    let selected_tab = ui_state.selected_tab;
    let selected_button = ui_state.selected_button;
    let gauge_value = ui_state.gauge_value;
    let counter = ui_state.counter;
    let sparkline_data = ui_state.sparkline_data.clone();
    let mut list_state = ui_state.list_state.clone();
    let mouse_position = ui_state.mouse_position;

    // Variables to capture layout rectangles
    let mut button_rects = Vec::new();
    let mut h_button_rects = Vec::new();
    let mut list_inner_rect = None;
    let mut gauge_inner_rect = None;

    let rotation_angle = (time.elapsed_secs() * 0.8).sin() * 45.0;

    ui_state.terminal.draw_and_render(
        &render_device,
        &render_queue,
        &mut images,
        &mut materials,
        &marker_query,
        |frame| {
            let area = frame.area();

            let tabs = Tabs::new(vec!["Buttons", "Lists", "Charts", "Interactive", "Glyphs"])
                .block(
                    Block::bordered()
                        .title(format!("WASM Widget Catalog | Rot: {:.1}deg", rotation_angle)),
                )
                .style(Style::default().fg(RatatuiColor::White))
                .highlight_style(Style::default().fg(RatatuiColor::Yellow).bold())
                .select(selected_tab)
                .divider("|");

            let chunks = Layout::default()
                .direction(Direction::Vertical)
                .constraints([
                    Constraint::Length(1),
                    Constraint::Length(3),
                    Constraint::Min(0),
                ])
                .split(area);

            let ruler = (0..100)
                .map(|i| {
                    if i % 10 == 0 {
                        (i / 10).to_string().chars().next().unwrap()
                    } else if i % 5 == 0 {
                        '|'
                    } else {
                        '.'
                    }
                })
                .collect::<String>();
            frame.render_widget(
                Paragraph::new(ruler).style(Style::default().fg(RatatuiColor::DarkGray)),
                chunks[0],
            );

            frame.render_widget(tabs, chunks[1]);

            match selected_tab {
                0 => {
                    let (btn_rects, h_btn_rects) =
                        draw_buttons_tab(frame, chunks[2], selected_button, counter, gauge_value);
                    button_rects = btn_rects;
                    h_button_rects = h_btn_rects;
                }
                1 => {
                    list_inner_rect = Some(draw_lists_tab(frame, chunks[2], &mut list_state));
                }
                2 => draw_charts_tab(frame, chunks[2], gauge_value, counter, &sparkline_data),
                3 => {
                    gauge_inner_rect = Some(draw_interactive_tab(frame, chunks[2], gauge_value));
                }
                4 => draw_glyphs_tab(frame, chunks[2]),
                _ => {}
            }

            let mouse_info = if let Some((col, row)) = mouse_position {
                format!(" Mouse: col={}, row={}", col, row)
            } else {
                " Mouse: -".to_string()
            };

            let status = Paragraph::new(format!(
                " Counter: {} | Gauge: {}% | Tab: {} |{} | WASM 3D Rotating!",
                counter,
                gauge_value,
                selected_tab + 1,
                mouse_info
            ))
            .style(
                Style::default()
                    .bg(RatatuiColor::Green)
                    .fg(RatatuiColor::Black),
            );

            let status_area = ratatui::layout::Rect {
                x: area.x,
                y: area.bottom().saturating_sub(1),
                width: area.width,
                height: 1,
            };
            frame.render_widget(status, status_area);
        },
    );

    // Store captured layout rectangles for hit testing
    ui_state.button_rects = button_rects;
    ui_state.h_button_rects = h_button_rects;
    ui_state.list_inner_rect = list_inner_rect;
    ui_state.gauge_inner_rect = gauge_inner_rect;
    ui_state.list_state = list_state;
}

fn draw_buttons_tab(
    frame: &mut ratatui::Frame,
    area: ratatui::layout::Rect,
    selected_button: Option<usize>,
    counter: usize,
    gauge_value: u16,
) -> (Vec<ratatui::layout::Rect>, Vec<ratatui::layout::Rect>) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3),
            Constraint::Length(3),
            Constraint::Length(3),
            Constraint::Length(5),
            Constraint::Min(0),
        ])
        .margin(1)
        .split(area);

    let button_labels = ["Increment Counter", "Increase Gauge", "Decrease Gauge"];

    for (i, label) in button_labels.iter().enumerate() {
        let is_selected = selected_button == Some(i);
        let style = if is_selected {
            Style::default()
                .bg(RatatuiColor::Yellow)
                .fg(RatatuiColor::Black)
                .bold()
        } else {
            Style::default()
                .bg(RatatuiColor::DarkGray)
                .fg(RatatuiColor::White)
        };

        let button = Paragraph::new(format!("  {}  ", label))
            .style(style)
            .block(Block::bordered());

        frame.render_widget(button, chunks[i]);
    }

    let horizontal_area = chunks[3];
    let h_chunks = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Percentage(20),
            Constraint::Percentage(20),
            Constraint::Percentage(20),
            Constraint::Percentage(20),
            Constraint::Percentage(20),
        ])
        .split(horizontal_area);

    let h_labels = ["Button 1", "ボタン 2", "按鈕 3", "botón 4", "düğme 5"];
    for (i, label) in h_labels.iter().enumerate() {
        let is_selected = selected_button == Some(i + 3);
        let style = if is_selected {
            Style::default()
                .bg(RatatuiColor::Cyan)
                .fg(RatatuiColor::Black)
                .bold()
        } else {
            Style::default()
                .bg(RatatuiColor::Blue)
                .fg(RatatuiColor::White)
        };

        let button = Paragraph::new(format!(" {} ", label))
            .style(style)
            .alignment(Alignment::Center)
            .block(Block::bordered());

        frame.render_widget(button, h_chunks[i]);
    }

    let selected_info = if let Some(idx) = selected_button {
        if idx < 3 {
            format!("Last: Vertical button {}", idx + 1)
        } else {
            format!("Last: Horizontal button {}", idx - 2)
        }
    } else {
        "Last: None".to_string()
    };

    let info = Paragraph::new(vec![
        Line::from(""),
        Line::from("Click buttons with mouse on rotating 3D plane!")
            .style(Style::default().fg(RatatuiColor::Cyan)),
        Line::from(format!("Current counter: {}", counter)),
        Line::from(format!("Current gauge: {}%", gauge_value)),
        Line::from(selected_info).style(Style::default().fg(RatatuiColor::Yellow)),
    ])
    .block(Block::bordered().title("Info"));

    frame.render_widget(info, chunks[4]);

    // Return button rectangles for hit testing
    let button_rects = chunks.iter().take(3).cloned().collect();
    let h_button_rects = h_chunks.to_vec();

    (button_rects, h_button_rects)
}

fn draw_lists_tab(
    frame: &mut ratatui::Frame,
    area: ratatui::layout::Rect,
    list_state: &mut ListState,
) -> ratatui::layout::Rect {
    let items: Vec<ListItem> = (0..10)
        .map(|i| {
            let content = format!("Item {} - Click to select", i + 1);
            ListItem::new(content)
        })
        .collect();

    let list = List::new(items)
        .block(Block::bordered().title("Selectable List"))
        .highlight_style(
            Style::default()
                .bg(RatatuiColor::Yellow)
                .fg(RatatuiColor::Black)
                .add_modifier(Modifier::BOLD),
        )
        .highlight_symbol(">> ");

    frame.render_stateful_widget(list, area, list_state);

    // Return inner area for hit testing
    ratatui::layout::Rect {
        x: area.x + 1,
        y: area.y + 1,
        width: area.width.saturating_sub(2),
        height: area.height.saturating_sub(2),
    }
}

fn draw_charts_tab(
    frame: &mut ratatui::Frame,
    area: ratatui::layout::Rect,
    gauge_value: u16,
    counter: usize,
    sparkline_data: &[u64],
) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Percentage(50), Constraint::Percentage(50)])
        .margin(1)
        .split(area);

    let data = [
        ("Counter", counter as u64),
        ("Gauge", gauge_value as u64),
        ("Items", 10),
        ("Tabs", 4),
    ];

    let barchart = BarChart::default()
        .block(Block::bordered().title("Bar Chart"))
        .data(
            BarGroup::default().bars(
                &data
                    .iter()
                    .map(|(label, value)| Bar::default().value(*value).label((*label).into()))
                    .collect::<Vec<_>>(),
            ),
        )
        .bar_width(9)
        .bar_gap(2)
        .bar_style(Style::default().fg(RatatuiColor::Yellow))
        .value_style(
            Style::default()
                .fg(RatatuiColor::Black)
                .bg(RatatuiColor::Yellow),
        );

    frame.render_widget(barchart, chunks[0]);

    let sparkline = Sparkline::default()
        .block(Block::bordered().title("Sparkline (Auto-scrolling)"))
        .data(sparkline_data)
        .style(Style::default().fg(RatatuiColor::Green));

    frame.render_widget(sparkline, chunks[1]);
}

fn draw_interactive_tab(
    frame: &mut ratatui::Frame,
    area: ratatui::layout::Rect,
    gauge_value: u16,
) -> ratatui::layout::Rect {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(5),
            Constraint::Length(5),
            Constraint::Min(0),
        ])
        .margin(1)
        .split(area);

    let gauge = Gauge::default()
        .block(Block::bordered().title("Interactive Gauge (Click to adjust)"))
        .gauge_style(
            Style::default()
                .fg(RatatuiColor::Cyan)
                .bg(RatatuiColor::Black),
        )
        .percent(gauge_value);

    frame.render_widget(gauge, chunks[0]);

    let line_gauge = LineGauge::default()
        .block(Block::bordered().title("Line Gauge"))
        .filled_style(Style::default().fg(RatatuiColor::Magenta))
        .line_set(symbols::line::THICK)
        .ratio(gauge_value as f64 / 100.0);

    frame.render_widget(line_gauge, chunks[1]);

    let instructions = Paragraph::new(vec![
        Line::from(""),
        Line::from("Mouse Controls (3D Ray Casting):")
            .style(Style::default().fg(RatatuiColor::Yellow).bold()),
        Line::from("  - Click tabs to switch"),
        Line::from("  - Click gauge bar to set value"),
        Line::from("  - Click buttons to interact"),
        Line::from("  - Click list items to select"),
        Line::from(""),
        Line::from("Keyboard Controls:").style(Style::default().fg(RatatuiColor::Yellow).bold()),
        Line::from("  - Tab: Switch tabs"),
        Line::from("  - Left/Right: Adjust gauge"),
        Line::from("  - Up/Down: Navigate list (in Lists tab)"),
    ])
    .block(Block::bordered().title("Help"));

    frame.render_widget(instructions, chunks[2]);

    // Return gauge inner area for hit testing
    ratatui::layout::Rect {
        x: chunks[0].x + 1,
        y: chunks[0].y + 1,
        width: chunks[0].width.saturating_sub(2),
        height: chunks[0].height.saturating_sub(2),
    }
}

fn draw_glyphs_tab(frame: &mut ratatui::Frame, area: ratatui::layout::Rect) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3), // Box Drawing
            Constraint::Length(3), // Block Elements
            Constraint::Length(5), // Braille
            Constraint::Length(3), // Powerline
            Constraint::Min(0),    // Info
        ])
        .margin(1)
        .split(area);

    // Box Drawing
    let box_lines = vec![Line::from(vec![
        Span::raw("Box: "),
        Span::styled(
            "─│┌┐└┘├┤┬┴┼ ━┃┏┓┗┛ ═║╔╗╚╝╠╣╦╩╬ ╭╮╯╰",
            Style::default().fg(RatatuiColor::Cyan),
        ),
    ])];
    let box_para = Paragraph::new(box_lines).block(Block::bordered().title("Box Drawing"));
    frame.render_widget(box_para, chunks[0]);

    // Block Elements
    let block_lines = vec![Line::from(vec![
        Span::raw("Block: "),
        Span::styled(
            "░▒▓█ ▀▄▌▐ ▁▂▃▄▅▆▇ ▏▎▍▊ ▖▗▘▝▚▞",
            Style::default().fg(RatatuiColor::Green),
        ),
    ])];
    let block_para = Paragraph::new(block_lines).block(Block::bordered().title("Block Elements"));
    frame.render_widget(block_para, chunks[1]);

    // Braille
    let braille_lines = vec![
        Line::from(vec![Span::styled(
            "⠀⠁⠂⠃⠄⠅⠆⠇ ⠈⠉⠊⠋⠌⠍⠎⠏ ⠐⠑⠒⠓⠔⠕⠖⠗",
            Style::default().fg(RatatuiColor::Magenta),
        )]),
        Line::from(vec![Span::styled(
            "⠘⠙⠚⠛⠜⠝⠞⠟ ⠠⠡⠢⠣⠤⠥⠦⠧ ⡀⡁⡂⡃⡄⡅⡆⡇",
            Style::default().fg(RatatuiColor::Magenta),
        )]),
        Line::from(vec![
            Span::styled("⣿ ", Style::default().fg(RatatuiColor::Magenta)),
            Span::raw("(All dots)"),
        ]),
    ];
    let braille_para =
        Paragraph::new(braille_lines).block(Block::bordered().title("Braille Patterns"));
    frame.render_widget(braille_para, chunks[2]);

    // Powerline
    let powerline_lines = vec![Line::from(vec![
        Span::raw("Powerline: "),
        Span::styled(
            "\u{E0B0}\u{E0B1}\u{E0B2}\u{E0B3} \u{E0B4}\u{E0B5}\u{E0B6}\u{E0B7} \u{E0B8}\u{E0B9}\u{E0BA}\u{E0BB}",
            Style::default().fg(RatatuiColor::Blue),
        ),
    ])];
    let powerline_para =
        Paragraph::new(powerline_lines).block(Block::bordered().title("Powerline Symbols"));
    frame.render_widget(powerline_para, chunks[3]);

    // Info
    let info = Paragraph::new(vec![
        Line::from(""),
        Line::from("All glyphs above are programmatically rendered")
            .style(Style::default().fg(RatatuiColor::Yellow)),
        Line::from("using tiny-skia and pre-baked into the texture atlas."),
        Line::from(""),
        Line::from("This provides pixel-perfect rendering with zero"),
        Line::from("runtime overhead. Running in WebAssembly!"),
    ])
    .block(Block::bordered().title("Info"));
    frame.render_widget(info, chunks[4]);
}

/// System that rotates the plane in seesaw motion for always-visible interaction
fn rotate_plane(time: Res<Time>, mut query: Query<&mut Transform, With<RotatingPlane>>) {
    for mut transform in &mut query {
        // Seesaw rotation: oscillate +/-45 degrees around Z axis
        let angle = (time.elapsed_secs() * 0.8).sin() * std::f32::consts::FRAC_PI_4;
        transform.rotation =
            Quat::from_rotation_x(std::f32::consts::FRAC_PI_2) * Quat::from_rotation_z(angle);
    }
}
